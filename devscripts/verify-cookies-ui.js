(async () => {
  const wait = () => new Promise(resolve => setTimeout(resolve, 160));
  const check = (value, message) => { if (!value) throw new Error(message); };
  const click = selector => { const node = document.querySelector(selector); check(node, selector); node.click(); };
  const input = document.querySelector('[data-url-input]');
  input.value = 'https://www.bilibili.com/video/BV1R1e4zKEh1/';
  input.dispatchEvent(new Event('input', {bubbles:true}));
  click('[data-action="analyze"]'); await wait(); await wait();
  check(document.querySelector('.parser-notice')?.textContent.includes('1080p'), 'Current resolution is visible');
  click('.parser-notice [data-action="open-cookie-settings"]'); await wait();
  check(document.querySelector('[data-setting="cookiesBrowser"]'), 'Direct navigation to network settings');
  check(!document.querySelector('.parser-dialog'), 'Parser does not overlap settings');
  const set = (key, value) => {
    const node = document.querySelector(`[data-setting="${key}"]`);
    node.value = value; node.dispatchEvent(new Event('input', {bubbles:true}));
  };
  set('cookiesBrowser', 'chrome');
  set('cookieText', 'SESSDATA=ui-fixture-only; bili_jct=fixture');
  click('[data-action="save-settings"]'); await wait(); await wait();
  check(!document.querySelector('.modal'), 'Save finishes');
  click('[data-action="open-settings"]'); await wait();
  click('[data-action="settings-tab"][data-tab="network"]'); await wait();
  check(document.querySelector('[data-setting="cookiesFile"]').value.includes('cookies.txt'), 'Imported file selected');
  check(document.querySelector('[data-setting="cookiesBrowser"]').value === '', 'Browser extraction disabled after paste import');
  check(document.querySelector('[data-setting="cookieText"]').value === '', 'Raw Cookie cleared after save');
  set('cookieSite', 'douyin');
  set('cookieText', 'ttwid=ui-visitor-fixture; __ac_nonce=ui-nonce');
  click('[data-action="save-settings"]'); await wait(); await wait();
  check(!document.querySelector('.modal'), 'Douyin Cookie import finishes');
  click('[data-action="open-settings"]'); await wait();
  click('[data-action="settings-tab"][data-tab="network"]'); await wait();
  check(document.querySelector('[data-setting="cookieText"]').value === '', 'Douyin Cookie cleared after save');
  return {qualityExplanation:true,networkShortcut:true,cookieImport:true,douyinCookieImport:true,rawCookieCleared:true};
})()
