import { describe, it, expect } from "vitest";
import { slugify, transliterate } from "../lib/slugify";

describe("transliterate", () => {
  it("maps ё to yo (Cyrillic map runs before NFD)", () => {
    expect(transliterate("ёлка")).toBe("yolka");
    expect(transliterate("Ёж")).toBe("yozh");
  });

  it("maps ї to yi and й to y without NFD degradation", () => {
    expect(transliterate("їжак")).toBe("yizhak");
    expect(transliterate("май")).toBe("may");
  });

  it("transliterates Russian text", () => {
    expect(transliterate("Привет мир")).toBe("privet mir");
    expect(transliterate("щука")).toBe("shchuka");
  });

  it("drops hard and soft signs", () => {
    expect(transliterate("объект")).toBe("obekt");
    expect(transliterate("соль")).toBe("sol");
  });

  it("strips diacritics from Latin", () => {
    expect(transliterate("café über")).toBe("cafe uber");
  });

  it("keeps unmapped non-ASCII characters out of the Cyrillic path", () => {
    expect(transliterate("日本")).toBe("日本");
  });

  it("returns empty string for empty input", () => {
    expect(transliterate("")).toBe("");
  });
});

describe("slugify", () => {
  it("replaces spaces with dashes", () => {
    expect(slugify("fix login bug")).toBe("fix-login-bug");
  });

  it("collapses runs of special characters into one dash", () => {
    expect(slugify("a!!  @#b___c")).toBe("a-b-c");
  });

  it("trims leading and trailing dashes", () => {
    expect(slugify("  --Hello, World!--  ")).toBe("hello-world");
  });

  it("transliterates Cyrillic into a slug", () => {
    expect(slugify("Тесты для slugify")).toBe("testy-dlya-slugify");
    expect(slugify("Ёлка и їжак")).toBe("yolka-i-yizhak");
  });

  it("keeps digits", () => {
    expect(slugify("Task 42: v2")).toBe("task-42-v2");
  });

  it("returns empty string for empty input", () => {
    expect(slugify("")).toBe("");
  });

  it("returns empty string when nothing survives transliteration", () => {
    expect(slugify("日本語 🚀")).toBe("");
    expect(slugify("!!! ???")).toBe("");
  });
});
