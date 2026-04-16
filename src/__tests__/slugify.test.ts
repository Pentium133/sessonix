import { describe, it, expect } from "vitest";
import { slugify, transliterate } from "../lib/slugify";

describe("transliterate", () => {
  it("passes through ASCII lowercase", () => {
    expect(transliterate("hello world")).toBe("hello world");
  });

  it("lowercases input", () => {
    expect(transliterate("MixedCase")).toBe("mixedcase");
  });

  it("strips Latin diacritics", () => {
    expect(transliterate("café")).toBe("cafe");
    expect(transliterate("über")).toBe("uber");
    expect(transliterate("naïve façade")).toBe("naive facade");
  });

  it("maps Russian Cyrillic to Latin", () => {
    expect(transliterate("Привет мир")).toBe("privet mir");
    expect(transliterate("Исправить баг")).toBe("ispravit bag");
    expect(transliterate("Щенок жуёт")).toBe("shchenok zhuyot");
  });

  it("maps Ukrainian-specific letters", () => {
    expect(transliterate("їжак")).toBe("yizhak");
    expect(transliterate("єдність")).toBe("yednist");
  });

  it("drops unmapped scripts (CJK, emoji)", () => {
    // CJK collapses; only ASCII remains
    expect(transliterate("你好 world")).toMatch(/world/);
    expect(transliterate("🚀 launch")).toMatch(/launch/);
  });
});

describe("slugify", () => {
  it("produces kebab-case from ASCII", () => {
    expect(slugify("Fix auth flow")).toBe("fix-auth-flow");
  });

  it("transliterates Cyrillic before slugging", () => {
    expect(slugify("Исправить нотификации")).toBe("ispravit-notifikatsii");
    expect(slugify("Привет, мир!")).toBe("privet-mir");
  });

  it("handles accented Latin", () => {
    expect(slugify("Résumé builder")).toBe("resume-builder");
  });

  it("collapses multiple separators", () => {
    expect(slugify("a   b___c!!!d")).toBe("a-b-c-d");
  });

  it("trims leading/trailing separators", () => {
    expect(slugify("---hello---")).toBe("hello");
    expect(slugify("!!!tag")).toBe("tag");
  });

  it("returns empty string for unrepresentable input", () => {
    expect(slugify("")).toBe("");
    expect(slugify("🎉🎊")).toBe("");
    expect(slugify("中文")).toBe("");
  });
});
