// Minimal Playwright-shaped facade for the existing browser checks. It runs in
// the page and delegates only browser-process operations to the Python CDP host.
(() => {
  let rpcId = 0;
  const pending = new Map();
  window.__resolveReaderUxHost = (id, value, error) => {
    const entry = pending.get(id);
    if (!entry) return;
    pending.delete(id);
    error ? entry.reject(new Error(error)) : entry.resolve(value);
  };
  window.__readerUxHost = (method, args = {}) => new Promise((resolve, reject) => {
    const id = ++rpcId;
    pending.set(id, {resolve, reject});
    window.__readerUxBridge(JSON.stringify({id, method, args}));
  });

  const visible = node => !!node && !node.hidden && getComputedStyle(node).display !== 'none' && getComputedStyle(node).visibility !== 'hidden';
  const text = node => (node?.innerText ?? node?.textContent ?? '').trim();
  const accessibleName = node => (node.getAttribute('aria-label') || node.getAttribute('title') || text(node)).trim();
  const targetWindow = () => {
    const frame = document.querySelector('#reader-fixture');
    if (!frame?.contentWindow) throw new Error('reader fixture frame is unavailable');
    return frame.contentWindow;
  };

  class Locator {
    constructor(resolve) { this.resolve = resolve; }
    nodes() { return this.resolve().filter(Boolean); }
    first() { return new Locator(() => this.nodes().slice(0, 1)); }
    last() { return new Locator(() => this.nodes().slice(-1)); }
    nth(index) { return new Locator(() => this.nodes().slice(index, index + 1)); }
    async all() { return this.nodes().map((_, index) => this.nth(index)); }
    async count() { return this.nodes().length; }
    locator(selector) { return new Locator(() => this.nodes().flatMap(node => [...node.querySelectorAll(selector)])); }
    getByRole(role, options = {}) {
      const selector = role === 'button' ? 'button,[role="button"]' : role === 'tab' ? '[role="tab"]' : `${role},[role="${role}"]`;
      return new Locator(() => this.nodes().flatMap(root => [...root.querySelectorAll(selector)]).filter(node => {
        if (options.name === undefined) return true;
        const name = accessibleName(node);
        return options.exact ? name === options.name : name.toLowerCase().includes(String(options.name).toLowerCase());
      }));
    }
    getByText(needle, options = {}) {
      return new Locator(() => this.nodes().flatMap(root => [...root.querySelectorAll('*')]).filter(node => {
        const value = text(node);
        const match = options.exact ? value === needle : value.includes(needle);
        return match && ![...node.children].some(child => (options.exact ? text(child) === needle : text(child).includes(needle)));
      }));
    }
    filter(options = {}) {
      return new Locator(() => this.nodes().filter(node => {
        if (options.hasText !== undefined && !text(node).includes(String(options.hasText))) return false;
        if (options.has && !options.has.nodes().some(child => node.contains(child))) return false;
        return true;
      }));
    }
    async click(options = {}) {
      const node = this.nodes()[0];
      if (!node) throw new Error('click target not found');
      const modifiers = options.modifiers || [];
      if (!visible(node)) throw new Error('click target is not visible');
      let box = node.getBoundingClientRect();
      const target = targetWindow();
      if (box.top < 0 || box.left < 0 || box.bottom > target.innerHeight || box.right > target.innerWidth) {
        node.scrollIntoView({block: 'center', inline: 'center'});
        box = node.getBoundingClientRect();
      }
      return window.__readerUxHost('click', {x: box.x + box.width / 2, y: box.y + box.height / 2, modifiers});
    }
    async fill(value) {
      const node = this.nodes()[0];
      if (!node) throw new Error('fill target not found');
      node.focus();
      if ('value' in node) {
        node.value = '';
        node.dispatchEvent(new Event('input', {bubbles: true}));
        if (node.setSelectionRange) node.setSelectionRange(0, 0);
        if (value) await window.__readerUxHost('insertText', {text: value});
        node.dispatchEvent(new Event('change', {bubbles: true}));
      } else {
        node.textContent = value;
        node.dispatchEvent(new Event('input', {bubbles: true}));
        node.dispatchEvent(new Event('change', {bubbles: true}));
      }
    }
    async type(value) { await this.focus(); await keyboard.type(value); }
    async press(key) { await this.focus(); await keyboard.press(key); }
    async focus() { const node = this.nodes()[0]; if (!node) throw new Error('focus target not found'); node.focus(); }
    async selectOption(value) {
      const node = this.nodes()[0]; if (!node) throw new Error('select target not found');
      node.value = value; node.dispatchEvent(new Event('input', {bubbles: true})); node.dispatchEvent(new Event('change', {bubbles: true}));
    }
    async uncheck() {
      const node = this.nodes()[0]; if (!node) throw new Error('checkbox not found');
      node.checked = false; node.dispatchEvent(new Event('input', {bubbles: true})); node.dispatchEvent(new Event('change', {bubbles: true}));
    }
    async isVisible() { return visible(this.nodes()[0]); }
    async isHidden() { return !visible(this.nodes()[0]); }
    async isEnabled() { const node = this.nodes()[0]; return !!node && !node.disabled; }
    async isDisabled() { const node = this.nodes()[0]; return !node || !!node.disabled; }
    async innerText() { const node = this.nodes()[0]; if (!node) throw new Error('text target not found'); return text(node); }
    async inputValue() { const node = this.nodes()[0]; if (!node) throw new Error('input target not found'); return node.value; }
    async getAttribute(name) { return this.nodes()[0]?.getAttribute(name) ?? null; }
    async boundingBox() {
      const node = this.nodes()[0]; if (!node) return null;
      const r = node.getBoundingClientRect(); return {x:r.x, y:r.y, width:r.width, height:r.height};
    }
    async evaluate(fn, arg) {
      const node = this.nodes()[0]; if (!node) throw new Error('evaluate target not found');
      const target = targetWindow();
      target.__readerUxEvaluateNode = node;
      target.__readerUxEvaluateArg = arg;
      return await target.eval(`(${fn.toString()})(window.__readerUxEvaluateNode, window.__readerUxEvaluateArg)`);
    }
  }

  const root = new Locator(() => [targetWindow().document]);
  const locator = selector => new Locator(() => [...targetWindow().document.querySelectorAll(selector)]);
  const byRole = (role, options = {}) => root.getByRole(role, options);
  const byText = (needle, options = {}) => root.getByText(needle, options);

  function keyParts(value) {
    const pieces = value.split('+');
    const key = pieces.pop();
    const mods = new Set(pieces.map(x => x.toLowerCase()));
    const keyName = ({' ': ' ', Esc: 'Escape', Left: 'ArrowLeft', Right: 'ArrowRight', Up: 'ArrowUp', Down: 'ArrowDown'})[key] || key;
    return {key:keyName, modifiers:[...mods]};
  }
  const keyboard = {
    async press(key) {
      const data = keyParts(key), target = targetWindow(), active = target.document.activeElement;
      window.__readerUxDebug ||= [];
      window.__readerUxDebug.push({op:'press', key, active:active?.tagName, id:active?.id, value:active?.value});
      const result = await window.__readerUxHost('key', {...data, action:'press'});
      const after = target.document.activeElement;
      window.__readerUxDebug.push({op:'pressed', key, active:after?.tagName, id:after?.id, value:after?.value});
      return result;
    },
    async down(key) { const data = keyParts(key); return window.__readerUxHost('key', {...data, action:'down'}); },
    async up(key) { const data = keyParts(key); return window.__readerUxHost('key', {...data, action:'up'}); },
    async type(value) {
      const target = targetWindow(), active = target.document.activeElement;
      window.__readerUxDebug ||= [];
      window.__readerUxDebug.push({op:'type', text:value, active:active?.tagName, id:active?.id, value:active?.value});
      const result = await window.__readerUxHost('type', {text:value});
      const after = target.document.activeElement;
      window.__readerUxDebug.push({op:'typed', active:after?.tagName, id:after?.id, value:after?.value});
      return result;
    },
  };

  window.__readerUxPage = {
    locator, getByRole:byRole, getByText:byText, keyboard,
    async evaluate(fn, arg) {
      const target = targetWindow();
      target.__readerUxEvaluateArg = arg;
      return await target.eval(`(${fn.toString()})(window.__readerUxEvaluateArg)`);
    },
    async waitForFunction(fn, ...args) {
      const deadline = Date.now() + 20000;
      while (Date.now() < deadline) { if (await this.evaluate(fn, args[0])) return; await new Promise(resolve => setTimeout(resolve, 20)); }
      throw new Error('waitForFunction timed out');
    },
    async setViewportSize({width, height}) { return window.__readerUxHost('viewport', {width, height}); },
    async reload() { return window.__readerUxHost('reload'); },
    async goto(url) { return window.__readerUxHost('navigate', {url}); },
    context() { return {grantPermissions: permissions => window.__readerUxHost('permissions', {permissions})}; },
    async screenshot({path}) { return window.__readerUxHost('screenshot', {path}); },
  };
})();
