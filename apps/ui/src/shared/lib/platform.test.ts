import { describe, expect, it } from "vitest";
import { isApplePhoneOrTablet } from "./platform";

describe("isApplePhoneOrTablet", () => {
  it("recognizes iPhone, iPad and iPadOS in desktop mode", () => {
    expect(isApplePhoneOrTablet({ userAgent: "Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X)", maxTouchPoints: 5 })).toBe(true);
    expect(isApplePhoneOrTablet({ userAgent: "Mozilla/5.0 (iPad; CPU OS 18_0 like Mac OS X)", maxTouchPoints: 5 })).toBe(true);
    expect(isApplePhoneOrTablet({ userAgent: "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)", maxTouchPoints: 5 })).toBe(true);
  });
  it("is false on a Mac, Android, Windows and without navigator", () => {
    expect(isApplePhoneOrTablet({ userAgent: "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)", maxTouchPoints: 0 })).toBe(false);
    expect(isApplePhoneOrTablet({ userAgent: "Mozilla/5.0 (Linux; Android 15)", maxTouchPoints: 5 })).toBe(false);
    expect(isApplePhoneOrTablet({ userAgent: "Mozilla/5.0 (Windows NT 10.0)", maxTouchPoints: 0 })).toBe(false);
    expect(isApplePhoneOrTablet(undefined)).toBe(false);
  });
});
