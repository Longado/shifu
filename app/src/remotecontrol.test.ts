import { describe, it, expect } from "vitest";
import { parsePairLink, parseSessionCatalog } from "../remote/channel";

const host = "1".repeat(32);
const credential = { id: "2".repeat(32), key: "3".repeat(64), expires: 2000000000 };
const fragment = btoa(JSON.stringify(credential));

describe("remote controller pairing boundary", () => {
  it("accepts typed creation choices and rejects malformed project or agent data", () => {
    const catalog = {projects: [{path: "/work/项目", name: "项目"}], agents: [{kind: "codex", name: "Codex", installed: true}]};
    expect(parseSessionCatalog(catalog)).toEqual(catalog);
    for (const value of [null, {}, {projects: [null], agents: []}, {projects: [], agents: [{kind: "shell", name: "Shell", installed: true}]}, {projects: [], agents: [{kind: "codex", name: "Codex", installed: "yes"}]}]) expect(() => parseSessionCatalog(value)).toThrow();
  });
  it("reads the remote host and one-time credential from its pairing address", () => {
    const parsed = parsePairLink(`https://relay.example/h/${host}#pair=${fragment}`);
    expect(parsed.target).toEqual({relay: "https://relay.example", host});
    expect(parsed.credential).toEqual(credential);
  });

  it("rejects a bare server address, plaintext public endpoint and invalid credential", () => {
    for (const link of ["not a URL", "https://relay.example/", `http://relay.example/h/${host}#pair=${fragment}`, `https://relay.example/h/${host}?token=secret#pair=${fragment}`, `https://relay.example/h/${host}#pair=${btoa(JSON.stringify({id: "wrong", key: "wrong"}))}`]) {
      expect(() => parsePairLink(link)).toThrow();
    }
  });
});
