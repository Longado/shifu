import { describe, expect, it } from "vitest";
import { decodeOsc52 } from "./clipboard";

// What tmux sends after a mouse selection: `ESC ] 52 ; c ; <base64 of UTF-8> BEL`; xterm.js hands us the part after `52;`.
describe("decodeOsc52", () => {
  it("decodes ASCII", () => {
    expect(decodeOsc52("c;aGVsbG8gc2hpZnU=")).toBe("hello shifu");
  });

  it("decodes Chinese as UTF-8, not Latin-1", () => {
    expect(decodeOsc52("c;5L2g5aW9IHNoaWZ1")).toBe("你好 shifu");
  });

  it("accepts an empty or multi-letter selection field", () => {
    expect(decodeOsc52(";aGk=")).toBe("hi");
    expect(decodeOsc52("pc;aGk=")).toBe("hi");
  });

  it("ignores clipboard queries and garbage", () => {
    expect(decodeOsc52("c;?")).toBeNull();
    expect(decodeOsc52("c;")).toBeNull();
    expect(decodeOsc52("c;not base64!")).toBeNull();
  });
});
