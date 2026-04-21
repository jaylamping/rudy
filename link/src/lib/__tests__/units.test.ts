import { describe, expect, it } from "vitest";
import {
  degToRad,
  formatAngleDeg,
  formatAngularVelDeg,
  isAngleUnit,
  isAngularVelUnit,
  radToDeg,
} from "@/lib/units";

describe("units", () => {
  it("radToDeg / degToRad round-trip", () => {
    expect(radToDeg(Math.PI)).toBeCloseTo(180, 6);
    expect(degToRad(180)).toBeCloseTo(Math.PI, 6);
    expect(radToDeg(degToRad(42))).toBeCloseTo(42, 6);
  });

  it("formatAngleDeg", () => {
    expect(formatAngleDeg(0)).toBe("0.00°");
    expect(formatAngleDeg(Math.PI / 2)).toBe("90.00°");
    expect(formatAngleDeg(null)).toBe("-");
    expect(formatAngleDeg(undefined)).toBe("-");
    expect(formatAngleDeg(NaN)).toBe("-");
    expect(formatAngleDeg(1, 4)).toBe(`${radToDeg(1).toFixed(4)}°`);
  });

  it("formatAngularVelDeg", () => {
    expect(formatAngularVelDeg(0)).toBe("0.0°/s");
    expect(formatAngularVelDeg(-0.5)).toMatch(/^-/);
    expect(formatAngularVelDeg(null)).toBe("-");
    expect(formatAngularVelDeg(NaN)).toBe("-");
  });

  it("isAngleUnit", () => {
    expect(isAngleUnit("rad")).toBe(true);
    expect(isAngleUnit("Radian")).toBe(true);
    expect(isAngleUnit("  radians  ")).toBe(true);
    expect(isAngleUnit("rad_per_s")).toBe(false);
    expect(isAngleUnit(null)).toBe(false);
  });

  it("isAngularVelUnit", () => {
    expect(isAngularVelUnit("rad_per_s")).toBe(true);
    expect(isAngularVelUnit("rad/s")).toBe(true);
    expect(isAngularVelUnit("Radians_Per_Second")).toBe(true);
    expect(isAngularVelUnit("rad")).toBe(false);
  });
});
