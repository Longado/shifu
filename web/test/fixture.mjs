import http from 'node:http';

const escape = value => String(value).replaceAll('&', '&amp;').replaceAll('"', '&quot;').replaceAll('<', '&lt;');
function html(values = {}, count = 0) {
  return `<!doctype html><html lang="zh-CN"><meta charset="utf-8"><title>网页协作测试</title>
  <style>body{font:16px -apple-system,BlinkMacSystemFont,"PingFang SC",sans-serif;margin:48px;color:#222}main{max-width:540px}h1{font-size:24px}label{display:block;margin:24px 0}input{display:block;box-sizing:border-box;width:100%;font:inherit;padding:8px;margin-top:8px;border:1px solid #aaa;border-radius:4px}button{font:inherit;padding:8px 16px;margin-right:12px}p{line-height:1.6}</style>
  <main><h1>网页协作测试</h1><p id="login"></p><form>
  <label>姓名<input name="name" value="${escape(values.name || '')}"></label>
  <label>备注<input name="note" value="${escape(values.note || '')}"></label>
  <button type="button" id="save">保存草稿</button><button type="button" id="slow">缓慢保存</button>
  <p role="status" id="status">已保存 ${count} 次</p></form></main>
  <script>
  if(!document.cookie.includes('fixture-session='))document.cookie='fixture-session='+crypto.randomUUID()+';path=/';
  document.querySelector('#login').textContent='模拟登录会话：'+document.cookie.split('fixture-session=')[1];
  const values=()=>({name:document.querySelector('[name=name]').value,note:document.querySelector('[name=note]').value});
  document.querySelector('#save').onclick=async()=>{const result=await(await fetch('/save',{method:'POST',body:JSON.stringify(values())})).json();document.querySelector('#status').textContent='已保存 '+result.count+' 次';};
  document.querySelector('#slow').onclick=()=>{navigator.sendBeacon('/mark',JSON.stringify(values()));location.href='/slow';};
  </script></html>`;
}

export async function fixture() {
  const saves = [];
  const waits = [];
  const slow = new Set();
  const server = http.createServer(async (request, response) => {
    if (request.method === 'POST') {
      let body = '';
      for await (const chunk of request) body += chunk;
      saves.push(JSON.parse(body));
      for (const wait of waits.splice(0)) { if (saves.length >= wait.count) wait.resolve(); else waits.push(wait); }
      response.setHeader('content-type', 'application/json');
      response.end(JSON.stringify({ count: saves.length }));
    } else if (request.url === '/groups') {
      response.setHeader('content-type', 'text/html; charset=utf-8');
      response.end('<main><section role="group" aria-label="第一组"><label>姓名<input></label></section><section role="group" aria-label="第二组"><label>姓名<input></label></section></main>');
    } else if (request.url === '/large') {
      response.setHeader('content-type', 'text/html; charset=utf-8');
      response.end('<main><h1>大页面开始</h1><p>' + '中文测试🙂'.repeat(8000) + '</p><button>大页面末尾</button></main>');
    } else if (request.url === '/read') {
      response.setHeader('content-type', 'text/html; charset=utf-8');
      response.end('<aside>SIDEBAR-ONLY</aside><main><h1>正文资料</h1><p>标签关闭返回 tab_gone，不会接管另一页。</p><p>字面量 [a+b]? 不是正则。</p><p id="live">ARTICLE-PRIVATE-FIXTURE 当前旧版本</p><label>姓名<input></label><button>保存草稿</button></main>');
    } else if (request.url === '/read-next') {
      response.setHeader('content-type', 'text/html; charset=utf-8');
      response.end('<main><p>ONLY-NEXT-PAGE 新页面的资料。</p></main>');
    } else if (request.url === '/read-huge') {
      response.setHeader('content-type', 'text/html; charset=utf-8');
      response.end('<main><h1>超限正文</h1><p>' + '过长正文🙂'.repeat(30000) + '</p></main>');
    } else if (request.url === '/stalled') {
      // Body text arrives, but a blocking script never does, so DOMContentLoaded never fires.
      response.setHeader('content-type', 'text/html; charset=utf-8');
      response.end('<main><h1>已到正文</h1><p>STALLED-BEFORE-SCRIPT 先到的资料。</p></main><script src="/slow"></script><p>脚本之后</p>');
    } else if (request.url === '/blocked-head') {
      // A stylesheet/script in <head> never arrives, so the browser never builds <body>.
      response.setHeader('content-type', 'text/html; charset=utf-8');
      response.end('<!doctype html><html><head><script src="/slow"></script><style>x{}</style></head><body><nav>NAV-ONLY</nav><main><h1>头部卡住</h1><p>HEAD-BLOCKED-TEXT 正文已到。</p><script>var HIDDEN_SCRIPT_TEXT=1</script></main></body></html>');
    } else if (request.url === '/slow') {
      slow.add(response);
      response.on('close', () => slow.delete(response));
    } else {
      response.setHeader('content-type', 'text/html; charset=utf-8');
      response.end(html(saves.at(-1), saves.length));
    }
  });
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  return {
    url: `http://127.0.0.1:${server.address().port}/`, saves,
    waitFor: count => saves.length >= count ? Promise.resolve() : new Promise(resolve => waits.push({ count, resolve })),
    releaseSlow: () => { for (const response of slow) { response.setHeader('content-type', 'text/html; charset=utf-8'); response.end(html(saves.at(-1), saves.length)); } },
    close: async () => { server.closeAllConnections(); await new Promise(resolve => server.close(resolve)); },
  };
}
