---
name: Slugify
description: Transliteration and git-ref-safe slug generation for task/branch names
targets:
  - ../src/lib/slugify.ts
---

# Slugify

- Cyrillic letters are mapped to Latin BEFORE NFD normalization, so `ё` → `yo`, `ї` → `yi`
  `[@test] ../src/__tests__/slugify.test.ts`
- Accented Latin is reduced to ASCII (`café` → `cafe`); unmapped scripts are dropped
  `[@test] ../src/__tests__/slugify.test.ts`
- `slugify` collapses spaces/special characters into single `-`, trims edge dashes, lowercases
  `[@test] ../src/__tests__/slugify.test.ts`
- Empty input, or input that transliterates to nothing, yields an empty string
  `[@test] ../src/__tests__/slugify.test.ts`
