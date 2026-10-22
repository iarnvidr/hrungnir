use reddit_offline::{library_bundle, Cache, Thread};
use std::{fs, io::Write, path::PathBuf};
use zip::{write::SimpleFileOptions, ZipWriter};
struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let mut b = [0; 8];
        getrandom::fill(&mut b).unwrap();
        let p = std::env::temp_dir().join(format!("library-test-{:x}", u64::from_ne_bytes(b)));
        fs::create_dir(&p).unwrap();
        Self(p)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn fixture() -> Thread {
    serde_json::from_str(include_str!("../fixtures/thread.json")).unwrap()
}
#[test]
fn saved_library_roundtrip_preserves_metadata_media_and_source() {
    let temp = Temp::new();
    let source = temp.0.join("source");
    fs::create_dir(&source).unwrap();
    let mut cache = Cache::open(source.join("cache.sqlite")).unwrap();
    let mut t = fixture();
    t.media_urls = vec!["https://i.redd.it/synthetic.jpg".into()];
    t.link_url = None;
    t.source = Some("proxy".into());
    cache.import(&t, 1024 * 1024).unwrap();
    let saved = cache.save_item(&t.id, "image", "Examples", true).unwrap();
    let dir = source.join("media").join(&t.id);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("image.jpg"), b"synthetic media bytes").unwrap();
    fs::write(
        dir.join("complete.json"),
        serde_json::to_vec(&serde_json::json!({"sources":t.media_urls,"files":["image.jpg"]}))
            .unwrap(),
    )
    .unwrap();
    let archive = temp.0.join("library.zip");
    library_bundle::export(&source, &archive).unwrap();
    let target = temp.0.join("target");
    library_bundle::import(&target, &archive).unwrap();
    let imported = Cache::open(target.join("cache.sqlite")).unwrap();
    assert_eq!(imported.read(&t.id).unwrap().unwrap(), t);
    assert_eq!(
        serde_json::to_value(imported.saved_items().unwrap()).unwrap(),
        serde_json::to_value(vec![saved]).unwrap()
    );
    assert_eq!(
        fs::read(target.join("media").join(&t.id).join("image.jpg")).unwrap(),
        b"synthetic media bytes"
    );
    library_bundle::import(&target, &archive).unwrap();
    fs::write(
        target.join("media").join(&t.id).join("image.jpg"),
        b"existing modified content",
    )
    .unwrap();
    assert!(library_bundle::import(&target, &archive).is_err());
    assert_eq!(
        fs::read(target.join("media").join(&t.id).join("image.jpg")).unwrap(),
        b"existing modified content"
    );
}
fn bad_archive(entries: &[(&str, &[u8])]) {
    let temp = Temp::new();
    let archive = temp.0.join("bad.zip");
    let mut zip = ZipWriter::new(fs::File::create(&archive).unwrap());
    for (name, bytes) in entries {
        zip.start_file(*name, SimpleFileOptions::default()).unwrap();
        zip.write_all(bytes).unwrap();
    }
    zip.finish().unwrap();
    let target = temp.0.join("target");
    assert!(library_bundle::import(&target, &archive).is_err());
    assert!(!target.join("cache.sqlite").exists());
    assert!(!temp.0.join("escape").exists());
}
#[test]
fn rejects_traversal_and_wrong_version_without_database_mutation() {
    bad_archive(&[("../escape", b"escape")]);
    bad_archive(&[(
        "manifest.json",
        br#"{"schema":2,"groups":[],"items":[],"threads":[],"media":{}}"#,
    )]);
    bad_archive(&[("manifest.json",br#"{"schema":1,"groups":[],"items":[],"threads":[],"media":{"t3_missing":["complete.json","file.jpg"]}}"#)]);
}
#[test]
fn rejects_unlisted_files() {
    bad_archive(&[
        (
            "manifest.json",
            br#"{"schema":1,"groups":[],"items":[],"threads":[],"media":{}}"#,
        ),
        ("secrets.txt", b"not in manifest"),
    ]);
}
#[test]
fn duplicate_zip_names_are_rejected_before_database_creation() {
    let temp = Temp::new();
    let path = temp.0.join("duplicate.zip");
    let mut zip = ZipWriter::new(fs::File::create(&path).unwrap());
    for name in ["manifest.json", "manifesu.json"] {
        zip.start_file(name, SimpleFileOptions::default()).unwrap();
        zip.write_all(br#"{"schema":1,"groups":[],"items":[],"threads":[],"media":{}}"#)
            .unwrap();
    }
    zip.finish().unwrap();
    let mut bytes = fs::read(&path).unwrap();
    let from = b"manifesu.json";
    for i in 0..=bytes.len() - from.len() {
        if &bytes[i..i + from.len()] == from {
            bytes[i..i + from.len()].copy_from_slice(b"manifest.json");
        }
    }
    fs::write(&path, bytes).unwrap();
    let target = temp.0.join("target");
    assert!(library_bundle::import(&target, &path).is_err());
    assert!(!target.exists());
}
#[cfg(unix)]
#[test]
fn export_rejects_media_symlinks() {
    let temp = Temp::new();
    let mut cache = Cache::open(temp.0.join("cache.sqlite")).unwrap();
    let mut t = fixture();
    t.link_url = None;
    t.media_urls = vec!["https://i.redd.it/synthetic.jpg".into()];
    cache.import(&t, 1024 * 1024).unwrap();
    cache.save_item(&t.id, "image", "Library", true).unwrap();
    let dir = temp.0.join("media").join(&t.id);
    fs::create_dir_all(&dir).unwrap();
    fs::write(temp.0.join("private"), b"must not export").unwrap();
    std::os::unix::fs::symlink(temp.0.join("private"), dir.join("image.jpg")).unwrap();
    fs::write(
        dir.join("complete.json"),
        serde_json::to_vec(&serde_json::json!({"sources":t.media_urls,"files":["image.jpg"]}))
            .unwrap(),
    )
    .unwrap();
    assert!(library_bundle::export(&temp.0, &temp.0.join("library.zip")).is_err());
}
#[test]
fn invalid_completed_manifest_does_not_create_database_or_media() {
    let temp = Temp::new();
    let mut t = fixture();
    t.link_url = None;
    t.media_urls = vec!["https://i.redd.it/synthetic.jpg".into()];
    let manifest = serde_json::to_vec(&serde_json::json!({"schema":1,"groups":[],
        "items":[{"id":t.id,"kind":"image","collection":"Library","keep_forever":true,"saved_at":100}],
        "threads":[t],"media":{(t.id.clone()):["complete.json","image.jpg"]}})).unwrap();
    let archive = temp.0.join("invalid-media.zip");
    let mut zip = ZipWriter::new(fs::File::create(&archive).unwrap());
    for (name, bytes) in [
        ("manifest.json".to_owned(), manifest),
        (
            format!("media/{}/complete.json", t.id),
            br#"{"sources":["https://i.redd.it/synthetic.jpg"],"files":["unlisted.jpg"]}"#.to_vec(),
        ),
        (format!("media/{}/image.jpg", t.id), b"image".to_vec()),
    ] {
        zip.start_file(name, SimpleFileOptions::default()).unwrap();
        zip.write_all(&bytes).unwrap();
    }
    zip.finish().unwrap();
    let target = temp.0.join("target");
    assert!(library_bundle::import(&target, &archive).is_err());
    assert!(!target.join("cache.sqlite").exists());
    assert!(!target.join("media").exists());
}
#[test]
fn explicit_linked_image_from_arbitrary_host_roundtrips() {
    let temp = Temp::new();
    let root = temp.0.join("source");
    fs::create_dir(&root).unwrap();
    let mut cache = Cache::open(root.join("cache.sqlite")).unwrap();
    let mut t = fixture();
    t.id = "t3_savedabcdef".into();
    t.comments.clear();
    t.missing.clear();
    t.source = Some("linked".into());
    t.media_urls.clear();
    t.link_url = Some("https://example.org/picture.png".into());
    cache.import(&t, 1024 * 1024).unwrap();
    cache.save_item(&t.id, "image", "Library", true).unwrap();
    let dir = root.join("media").join(&t.id);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("linked.png"), b"explicitly saved image").unwrap();
    fs::write(
        dir.join("complete.json"),
        serde_json::to_vec(&serde_json::json!({"sources":[t.link_url],"files":["linked.png"]}))
            .unwrap(),
    )
    .unwrap();
    let archive = temp.0.join("linked.zip");
    library_bundle::export(&root, &archive).unwrap();
    let target = temp.0.join("restored");
    library_bundle::import(&target, &archive).unwrap();
    assert_eq!(
        fs::read(target.join("media").join(&t.id).join("linked.png")).unwrap(),
        b"explicitly saved image"
    );
}
#[test]
fn importing_library_does_not_evict_existing_protected_content() {
    let temp = Temp::new();
    let source = temp.0.join("source");
    fs::create_dir(&source).unwrap();
    let mut imported = fixture();
    imported.id = "t3_imported".into();
    imported.comments.clear();
    imported.missing.clear();
    let mut cache = Cache::open(source.join("cache.sqlite")).unwrap();
    cache.import(&imported, 1024 * 1024).unwrap();
    cache
        .save_item(&imported.id, "post", "Library", true)
        .unwrap();
    let archive = temp.0.join("library.zip");
    library_bundle::export(&source, &archive).unwrap();
    let target = temp.0.join("target");
    fs::create_dir(&target).unwrap();
    let mut existing = Cache::open(target.join("cache.sqlite")).unwrap();
    let original = fixture();
    existing.import(&original, 1024 * 1024).unwrap();
    existing
        .save_item(&original.id, "post", "Original", true)
        .unwrap();
    library_bundle::import(&target, &archive).unwrap();
    assert_eq!(existing.read(&original.id).unwrap().unwrap(), original);
    assert!(existing.is_protected(&original.id).unwrap());
    assert!(existing.is_protected(&imported.id).unwrap());
}
#[test]
fn linked_article_without_images_roundtrips() {
    let temp = Temp::new();
    let root = temp.0.join("source");
    fs::create_dir(&root).unwrap();
    let mut cache = Cache::open(root.join("cache.sqlite")).unwrap();
    let mut t = fixture();
    t.id = "t3_savedarticle".into();
    t.comments.clear();
    t.missing.clear();
    t.source = Some("linked".into());
    t.media_urls.clear();
    t.link_url = Some("https://example.org/article".into());
    t.body = "Saved readable article text".into();
    cache.import(&t, 1024 * 1024).unwrap();
    cache.save_item(&t.id, "article", "Library", true).unwrap();
    let dir = root.join("media").join(&t.id);
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("complete.json"),
        serde_json::to_vec(&serde_json::json!({"sources":[t.link_url],"files":[]})).unwrap(),
    )
    .unwrap();
    let archive = temp.0.join("article.zip");
    library_bundle::export(&root, &archive).unwrap();
    let target = temp.0.join("restored");
    library_bundle::import(&target, &archive).unwrap();
    let restored = Cache::open(target.join("cache.sqlite"))
        .unwrap()
        .read(&t.id)
        .unwrap()
        .unwrap();
    assert_eq!(restored, t);
    assert!(target
        .join("media")
        .join(&t.id)
        .join("complete.json")
        .is_file());
}
#[test]
fn export_never_overwrites_existing_files_or_managed_application_paths() {
    let temp = Temp::new();
    let root = temp.0.join("data");
    fs::create_dir(&root).unwrap();
    let mut cache = Cache::open(root.join("cache.sqlite")).unwrap();
    cache.import(&fixture(), 1024 * 1024).unwrap();
    let before = fs::read(root.join("cache.sqlite")).unwrap();
    assert!(library_bundle::export(&root, &root.join("cache.sqlite")).is_err());
    assert_eq!(fs::read(root.join("cache.sqlite")).unwrap(), before);
    let existing = temp.0.join("backup.zip");
    fs::write(&existing, b"existing backup must survive").unwrap();
    assert!(library_bundle::export(&root, &existing).is_err());
    assert_eq!(fs::read(existing).unwrap(), b"existing backup must survive");
    for path in [
        "media/new.zip",
        "ca/new.zip",
        "browser/new.zip",
        "logs/new.zip",
        "cache.sqlite-wal",
    ] {
        let output = root.join(path);
        assert!(library_bundle::export(&root, &output).is_err());
        assert!(!output.exists());
    }
    library_bundle::export(&root, &root.join("linked-content.zip")).unwrap();
    assert!(root.join("linked-content.zip").is_file());
    assert!(library_bundle::export(&root, &root.join("media/../cache.sqlite")).is_err());
    assert_eq!(fs::read(root.join("cache.sqlite")).unwrap(), before);
}
