// Checks verifyChecksum against a stubbed https.get. Usage: node install.test.js
const https = require("https");
const crypto = require("crypto");
const { EventEmitter } = require("events");
const assert = require("assert");

const responses = {};
https.get = (url, _opts, cb) => {
  const res = new EventEmitter();
  const body = responses[url];
  res.statusCode = body === undefined ? 404 : 200;
  res.headers = {};
  res.resume = () => {};
  process.nextTick(() => {
    cb(res);
    if (body !== undefined) res.emit("data", Buffer.from(body));
    res.emit("end");
  });
  return new EventEmitter();
};

const { verifyChecksum } = require("./install");
const base = "https://example.test/rel";
const archive = "changesetter-x86_64-unknown-linux-gnu.tar.gz";
const data = Buffer.from("binary");
const sum = crypto.createHash("sha256").update(data).digest("hex");

(async () => {
  responses[`${base}/SHA256SUMS`] = `${"0".repeat(64)}  changesetter-aarch64-apple-darwin.tar.gz\n${sum}  ${archive}\n`;
  await verifyChecksum(base, archive, data, "0.9.0");

  responses[`${base}/SHA256SUMS`] = `${sum} *${archive}\n`;
  await verifyChecksum(base, archive, data, "0.9.0");

  await assert.rejects(verifyChecksum(base, archive, Buffer.from("tampered"), "0.9.0"), /Checksum mismatch/);

  responses[`${base}/SHA256SUMS`] = `${sum}  other.tar.gz\n`;
  await assert.rejects(verifyChecksum(base, archive, data, "0.9.0"), /no entry for/);

  delete responses[`${base}/SHA256SUMS`];
  // Older releases have no SHA256SUMS; 0.9.0 and later must.
  await verifyChecksum(base, archive, data, "0.8.0");
  await assert.rejects(verifyChecksum(base, archive, data, "0.9.0"), /HTTP 404/);
  await assert.rejects(verifyChecksum(base, archive, data, "1.0.0-rc.1"), /HTTP 404/);

  console.log("install checksum tests passed");
})().catch((e) => { console.error(e); process.exit(1); });
