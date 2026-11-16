#!/usr/bin/env python3
"""Exercise only owned, headless browsers against a temporary localhost fixture."""
import argparse
import json
import os
from pathlib import Path
import re
import subprocess
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import threading

PAGE = '''<!doctype html><meta charset="utf-8"><title>Shifu Browser Fixture</title>
<h1>Shifu Browser Fixture</h1><label>Message<textarea id="message"></textarea></label>
<button onclick="count.textContent=Number(count.textContent)+1">Increment</button><output id="count">0</output>
<label><input type="checkbox" id="enabled">Enabled</label>
<a href="/popup" target="_blank">Open popup</a>
<iframe title="Child frame" src="/frame"></iframe><output id="frameCount">0</output>
<script>addEventListener('message', e=>{if(e.data==='shifu-frame')frameCount.textContent=Number(frameCount.textContent)+1})</script>'''


class Fixture(BaseHTTPRequestHandler):
    def do_GET(self):
        page = PAGE
        if self.path == '/frame':
            page = '<!doctype html><button onclick="parent.postMessage(\'shifu-frame\',\'*\')">Frame action</button>'
        elif self.path == '/popup':
            page = '<!doctype html><title>Owned popup</title><h1>Owned popup</h1>'
        data = page.encode()
        self.send_response(200)
        self.send_header('Content-Type', 'text/html; charset=utf-8')
        self.send_header('Content-Length', str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def log_message(self, *_):
        pass


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('output', type=Path, help='new evidence directory')
    parser.add_argument('--cli', type=Path, required=True)
    parser.add_argument('--runtime', type=Path, default=Path.home() / '.shifu/browser/runtime')
    args = parser.parse_args()
    output = args.output.resolve()
    output.mkdir(mode=0o700)  # refuse to overwrite an earlier run
    cli = args.cli.resolve()
    env = dict(os.environ, SHIFU_BROWSER_HOME=str(output / 'browser-home'),
               SHIFU_BROWSER_RUNTIME=str(args.runtime.resolve()))
    env.pop('SHIFU_BROWSER_NODE', None)
    server = ThreadingHTTPServer(('127.0.0.1', 0), Fixture)
    worker = threading.Thread(target=server.serve_forever, daemon=True)
    worker.start()
    url = f'http://127.0.0.1:{server.server_port}/'
    sequence = 0
    opened = set()
    report = {'scope': 'owned_headless_localhost_fixture', 'checks': {}, 'cleanup': {}}

    def call(command, session='fixture', text=None, expect=True):
        nonlocal sequence
        sequence += 1
        process = subprocess.run([str(cli), 'browser', '--session', session, *command],
                                 input=text, text=True, capture_output=True, env=env, timeout=60)
        (output / f'{sequence:03d}-{command[0]}.json').write_text(process.stdout)
        value = json.loads(process.stdout)
        assert value.get('ok') is expect, (command[0], value, process.stderr)
        assert (process.returncode == 0) is expect, (command[0], process.returncode)
        return value

    def ref(role, name):
        path = output / f'snapshot-{sequence + 1:03d}.yml'
        call(['snapshot', f'--filename={path}'])
        tree = path.read_text()
        pattern = rf'{re.escape(role)} "{re.escape(name)}"[^\n]*?\[ref=([^\]]+)\]'
        found = re.search(pattern, tree)
        assert found, (role, name, tree)
        return found.group(1)

    def evaluate(code, session='fixture'):
        raw = call(['eval', code], session)['result']['result']
        if isinstance(raw, str):
            try:
                return json.loads(raw)
            except json.JSONDecodeError:
                return raw
        return raw

    try:
        opened.add('fixture')
        call(['open', url, '--persistent', '--headless'])
        call(['fill', ref('textbox', 'Message'), '--text-stdin'], text='中文消息\nsecond line')
        assert evaluate("() => document.querySelector('#message').value") == '中文消息\nsecond line'
        report['checks']['unicode_stdin_fill'] = True
        call(['fill', ref('textbox', 'Message'), '--text-stdin'], text='--session=literal-text')
        assert evaluate("() => document.querySelector('#message').value") == '--session=literal-text'
        call(['fill', ref('textbox', 'Message'), '--config=literal-text'])
        assert evaluate("() => document.querySelector('#message').value") == '--config=literal-text'
        call(['fill', ref('textbox', 'Message'), '--text-stdin'], text='中文消息\nsecond line')
        report['checks']['literal_text_is_not_a_provider_flag'] = True

        call(['click', ref('button', 'Increment')])
        assert evaluate("() => Number(document.querySelector('#count').textContent)") == 1
        report['checks']['single_click_and_dom_readback'] = True
        call(['click', 'e99999999'], expect=False)
        assert evaluate("() => Number(document.querySelector('#count').textContent)") == 1
        report['checks']['missing_ref_does_not_replay_action'] = True

        call(['check', ref('checkbox', 'Enabled')])
        assert evaluate("() => document.querySelector('#enabled').checked") is True
        call(['click', ref('button', 'Frame action')])
        assert evaluate("() => Number(document.querySelector('#frameCount').textContent)") == 1
        report['checks']['checkbox_and_iframe'] = True

        shot = call(['screenshot'])['result']['screenshot']['path']
        shot_path = Path(shot)
        assert shot_path.is_absolute() and shot_path.read_bytes().startswith(b'\x89PNG\r\n\x1a\n')
        report['screenshot'] = shot

        call(['click', ref('link', 'Open popup')])
        # The pinned official CLI returns tab-list as Markdown inside its JSON result.
        tabs = call(['tab-list'])['result']['result'].splitlines()
        tabs = [line for line in tabs if re.match(r'^- \d+: ', line)]
        assert len(tabs) == 2, tabs
        call(['tab-select', '0'])
        assert evaluate('() => document.title') == 'Shifu Browser Fixture'
        report['checks']['popup_and_tab_selection'] = True

        assert evaluate("() => {localStorage.setItem('shifu-fixture','kept'); return true;}") is True
        call(['snapshot'], session='other', expect=False)
        opened.add('other')
        call(['open', url, '--headless', '--ephemeral'], session='other')
        assert evaluate("() => localStorage.getItem('shifu-fixture')", session='other') is None
        report['checks']['session_isolation'] = True

        call(['close'])
        opened.remove('fixture')
        opened.add('fixture')
        call(['open', url, '--persistent', '--headless'])
        assert evaluate("() => localStorage.getItem('shifu-fixture')") == 'kept'
        report['checks']['persistent_profile_restart'] = True
        report['status'] = 'complete'
    except BaseException as error:
        report.update(status='failed', error=str(error))
        raise
    finally:
        for session in opened:
            try:
                result = call(['close'], session=session)
                report['cleanup'][session] = result['ok']
            except Exception as error:
                report['cleanup'][session] = str(error)
        server.shutdown()
        server.server_close()
        (output / 'report.json').write_text(json.dumps(report, ensure_ascii=False, indent=2) + '\n')
        print(output / 'report.json')


if __name__ == '__main__':
    main()
