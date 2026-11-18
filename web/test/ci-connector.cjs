// CI connector: `node test/ci-connector.cjs serve` starts one shared headless Chromium server;
// tests (and their observers) connect to that same browser, as they do locally.
const { chromium } = require('playwright-core');
const endpoint = 'ws://127.0.0.1:9339/pw';
const connect = () => chromium.connect(endpoint, { timeout: 8000 });
if (require.main === module && process.argv[2] === 'serve') {
  chromium.launchServer({ headless: true, host: '127.0.0.1', port: 9339, wsPath: 'pw' })
    .then(server => console.log(`browser server ${server.wsEndpoint()}`));
}
module.exports = { connect };
