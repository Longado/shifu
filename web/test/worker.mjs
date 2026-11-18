import { createRequire } from 'node:module';
import { createService } from '../service.mjs';
const { connect } = createRequire(import.meta.url)(process.env.SHIFU_TEST_CONNECTOR);
const service = await createService({ home: process.env.SHIFU_WEB_HOME, connect });
process.stdout.write(JSON.stringify({ ready: true, pid: process.pid }) + '\n');
process.on('SIGTERM', () => { void service.close(); });
await service.closed;
