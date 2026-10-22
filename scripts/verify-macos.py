#!/usr/bin/env python3
"""Check ARM64 architecture, system dylibs and ad-hoc code-directory hashes.

This is structural verification, not macOS Gatekeeper or launch validation.
"""
import hashlib
import pathlib
import plistlib
import struct
import sys
import json

root = pathlib.Path(__file__).resolve().parents[1]
config = json.loads((root / "src-tauri/tauri.conf.json").read_text())
app = pathlib.Path(sys.argv[1]) if len(sys.argv) > 1 else root / "artifacts" / f"{config['productName']}.app"
binary = app / "Contents/MacOS/offline-reader-desktop"
data = binary.read_bytes()
assert struct.unpack_from("<II", data) == (0xFEEDFACF, 0x0100000C), "Not ARM64 Mach-O"
signature = None
offset = 32
dylibs = []
for _ in range(struct.unpack_from("<I", data, 16)[0]):
    cmd, size = struct.unpack_from("<II", data, offset)
    assert size >= 8
    if cmd == 0x1D:
        start, length = struct.unpack_from("<II", data, offset + 8)
        signature = data[start:start + length]
    if cmd in (0xC, 0x80000018, 0x8000001F):
        name_offset = struct.unpack_from("<I", data, offset + 8)[0]
        name = data[offset + name_offset:offset + size].split(b"\0", 1)[0].decode()
        assert name.startswith(("/System/Library/", "/usr/lib/")), f"Unbundled dependency: {name}"
        dylibs.append(name)
    offset += size
assert signature is not None, "No code signature"
magic, length, count = struct.unpack_from(">III", signature)
assert magic == 0xFADE0CC0 and length <= len(signature)
blobs = {}
for index in range(count):
    slot, at = struct.unpack_from(">II", signature, 12 + index * 8)
    blob_length = struct.unpack_from(">I", signature, at + 4)[0]
    blobs[slot] = signature[at:at + blob_length]
directory = blobs[0]
_, _, _, flags, hash_offset, _, special, slots, limit = struct.unpack_from(">9I", directory)
hash_size, hash_type, _, page_exp = struct.unpack_from(">4B", directory, 36)
assert flags & 2, "Expected ad-hoc signature"
assert (hash_size, hash_type) == (32, 2), "Expected SHA-256"
page = 1 << page_exp
assert slots == (limit + page - 1) // page
for index in range(slots):
    actual = hashlib.sha256(data[index * page:min((index + 1) * page, limit)]).digest()
    expected = directory[hash_offset + index * 32:hash_offset + (index + 1) * 32]
    assert actual == expected, f"Code signature page mismatch: {index}"
for slot, contents in ((1, (app / "Contents/Info.plist").read_bytes()),
                       (2, blobs[2]),
                       (3, (app / "Contents/_CodeSignature/CodeResources").read_bytes())):
    assert slot <= special
    expected = directory[hash_offset - slot * 32:hash_offset - (slot - 1) * 32]
    assert hashlib.sha256(contents).digest() == expected, f"Special slot mismatch: {slot}"
info = plistlib.loads((app / "Contents/Info.plist").read_bytes())
assert info["CFBundleExecutable"] == binary.name
assert info["CFBundleDisplayName"] == config["productName"]
assert info["CFBundleIdentifier"] == config["identifier"]
assert info["CFBundleShortVersionString"] == config["version"]
assert info["CFBundleVersion"] == config["version"]
ats = info["NSAppTransportSecurity"]
assert not ats.get("NSAllowsArbitraryLoads", False)
assert not ats.get("NSAllowsArbitraryLoadsInWebContent", False)
assert set(ats.get("NSExceptionDomains", {})) == {"www.reddit.com"}
exception = ats["NSExceptionDomains"]["www.reddit.com"]
assert exception == {"NSIncludesSubdomains": False, "NSExceptionAllowsInsecureHTTPLoads": True}
assert binary.stat().st_mode & 0o111, "Missing executable bit"

def verify_helper(path):
    """Verify every signed slice of a standalone thin/universal helper."""
    raw=path.read_bytes()
    if struct.unpack_from('>I',raw)[0]==0xCAFEBABE:
        count=struct.unpack_from('>I',raw,4)[0]
        slices=[raw[start:start+size] for _,_,start,size,_ in (struct.unpack_from('>5I',raw,8+i*20) for i in range(count))]
    else:
        slices=[raw]
    digests=[];cpus=[]
    for code in slices:
        magic,cpu=struct.unpack_from('<II',code)
        assert magic==0xFEEDFACF and cpu in (0x0100000C,0x01000007),f'Unexpected helper architecture: {path}'
        cpus.append(cpu);at=32;signature=None
        for _ in range(struct.unpack_from('<I',code,16)[0]):
            command,size=struct.unpack_from('<II',code,at)
            assert size>=8
            if command==0x1D:
                start,length=struct.unpack_from('<II',code,at+8);signature=code[start:start+length]
            if command in (0xC,0x80000018,0x8000001F):
                start=struct.unpack_from('<I',code,at+8)[0]
                name=code[at+start:at+size].split(b'\0',1)[0].decode()
                assert name.startswith(('/System/Library/','/usr/lib/')),f'Unbundled helper dependency: {name}'
            at+=size
        assert signature is not None,f'Unsigned helper: {path}'
        magic,length,count=struct.unpack_from('>III',signature)
        assert magic==0xFADE0CC0 and length<=len(signature)
        directories=[]
        for i in range(count):
            slot,start=struct.unpack_from('>II',signature,12+i*8)
            if slot==0 or 0x1000<=slot<0x1005:
                length=struct.unpack_from('>I',signature,start+4)[0];directories.append(signature[start:start+length])
        assert directories
        for directory in directories:
            _,_,_,flags,hash_at,_,_,slots,limit=struct.unpack_from('>9I',directory)
            hash_size,hash_type,_,page_exp=struct.unpack_from('>4B',directory,36)
            assert flags&2 and (hash_size,hash_type)==(32,2)
            page=1<<page_exp
            assert slots==(limit+page-1)//page
            for i in range(slots):
                assert hashlib.sha256(code[i*page:min((i+1)*page,limit)]).digest()==directory[hash_at+i*32:hash_at+(i+1)*32],f'Helper page mismatch: {path}:{i}'
            digests.append(hashlib.sha256(directory).digest()[:20])
    assert 0x0100000C in cpus,f'Helper has no Apple Silicon slice: {path}'
    assert path.stat().st_mode&0o111
    return digests

helper_hashes={}
for name in ('yt-dlp','ffmpeg'):
    path=app/'Contents/Resources/bin'/name
    helper_hashes['Resources/bin/'+name]=verify_helper(path)
resources=plistlib.loads((app/'Contents/_CodeSignature/CodeResources').read_bytes())
verified_resources=0
for name,record in resources.get('files2',{}).items():
    path=app/'Contents'/name
    if not path.exists():path=app/'Contents/Resources'/name
    assert path.is_file(),f'Missing bundled resource: {name}'
    if 'hash2' in record:
        assert hashlib.sha256(path.read_bytes()).digest()==record['hash2'],f'Resource mismatch: {name}'
    elif 'hash' in record:
        assert hashlib.sha1(path.read_bytes()).digest()==record['hash'],f'Resource mismatch: {name}'
    elif 'cdhash' in record:
        key=str(path.relative_to(app/'Contents'))
        assert record['cdhash'] in helper_hashes.get(key,[]),f'Nested code mismatch: {name}'
    else:raise AssertionError(f'Unchecked resource entry: {name}')
    verified_resources+=1
print(f"PASS: ARM64 Mach-O, {len(dylibs)} system dependencies, {slots} signed code pages, Info.plist/requirements/resource hashes")
print(f"PASS: both media helpers' code pages/dependencies and {verified_resources} actual bundled resource entries")
print("macOS launch and Apple's codesign/Gatekeeper checks still require a Mac.")
