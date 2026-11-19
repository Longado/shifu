import { describe, expect, it } from "vitest";
import { dragNotches, pageNotches, PX_PER_NOTCH, scrolledBack, thumbTop, wheelNotches, wheelReport } from "./scrollstrip";

describe("pageNotches", () => {
  it("is about a screen: Claude speeds a burst of wheel steps up, the others move a line a step", () => {
    expect(pageNotches(40, true)).toBe(13);
    expect(pageNotches(40, false)).toBe(32);
    expect(pageNotches(5, false)).toBe(1);
    expect(pageNotches(2, true)).toBe(1);
  });
});

describe("scroll strip", () => {
  it("writes the wheel report a mouse would send, and reads its direction back", () => {
    expect(wheelReport(true, 40, 12)).toBe("\x1b[<64;40;12M");
    expect(wheelReport(false, 1, 1)).toBe("\x1b[<65;1;1M");
    expect(wheelNotches(wheelReport(true, 40, 12))).toBe(1);
    expect(wheelNotches(wheelReport(false, 40, 12))).toBe(-1);
    expect(wheelNotches("\x1b[<66;40;12M")).toBe(0); // sideways
    expect(wheelNotches("\x1b[<0;40;12M")).toBe(0); // a click
    expect(wheelNotches("\r")).toBe(0);
  });

  it("counts how far the view is scrolled back, inside what the track can show", () => {
    expect(scrolledBack(0, 1, 100)).toBe(1);
    expect(scrolledBack(0, -1, 100)).toBe(0); // already following
    expect(scrolledBack(99, 5, 100)).toBe(100); // the thumb stops at the top however long the conversation is
    expect(scrolledBack(100, -1, 100)).toBe(99);
  });

  it("rests the thumb at the bottom while following and lifts it as the view scrolls back", () => {
    const range = 300;
    expect(thumbTop(0, range)).toBe(300);
    expect(thumbTop(10, range)).toBe(300 - 10 * PX_PER_NOTCH);
    expect(thumbTop(1000, range)).toBe(0);
  });

  it("turns a drag into whole notches and keeps the remainder for the next move", () => {
    expect(dragNotches(PX_PER_NOTCH * 2 + 1)).toEqual({ notches: 2, rest: 1 });
    expect(dragNotches(-(PX_PER_NOTCH * 3 + 2))).toEqual({ notches: -3, rest: -2 });
    expect(dragNotches(PX_PER_NOTCH - 1)).toEqual({ notches: 0, rest: PX_PER_NOTCH - 1 });
  });
});
