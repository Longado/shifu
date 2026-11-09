import { describe, expect, it } from "vitest";
import { mouseReport } from "./mousereport";

describe("mouseReport", () => {
  it("tells wheel turns from other mouse reports and from keys", () => {
    expect(mouseReport("\x1b[<64;20;10M")).toBe("wheel"); // up
    expect(mouseReport("\x1b[<65;3;7M")).toBe("wheel"); // down
    expect(mouseReport("\x1b[<0;20;10M")).toBe("other"); // press
    expect(mouseReport("\x1b[<32;22;12M")).toBe("other"); // drag
    expect(mouseReport("\x1b[<0;22;12m")).toBe("other"); // release
    expect(mouseReport("\r")).toBeNull();
    expect(mouseReport("\x1b[B")).toBeNull();
    expect(mouseReport("1")).toBeNull();
  });
});
