import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { createRequire } from "node:module";
import test from "node:test";
import { transformWithEsbuild } from "vite";

const filename = fileURLToPath(new URL("../src/i18n.ts", import.meta.url));
const { code } = await transformWithEsbuild(readFileSync(filename, "utf8"), filename, { format: "cjs" });
const module = { exports: {} };
new Function("require", "module", "exports", code)(createRequire(filename), module, module.exports);
const { resolveLanguage, textsFor } = module.exports;

test("automatic language resolves supported Windows regional locales and falls back safely", () => {
  for (const [tag, expected] of [
    ["es-PE", "es"], ["en-GB", "en"], ["fr-CA", "fr"], ["zh-Hans-CN", "zh"],
    ["hi-IN", "hi"], ["ar-SA", "ar"], ["ar-EG", "ar"], ["bn-BD", "bn"],
    ["bn-IN", "bn"], ["pt-BR", "pt"], ["pt-PT", "pt"], ["PT_br", "pt"],
    ["de-DE", "en"], ["constructor", "en"], ["", "en"],
  ]) assert.equal(resolveLanguage("auto", tag), expected, tag);
});

test("an explicit language always wins over Windows without losing the choice", () => {
  for (const language of ["en", "es", "fr", "zh", "hi", "ar", "bn", "pt"]) {
    assert.equal(resolveLanguage(language, "de-DE"), language);
    assert.ok(textsFor(language).language);
  }
});
