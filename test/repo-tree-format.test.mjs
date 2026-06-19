import { test } from "node:test";
import assert from "node:assert/strict";
import { fileTypeLabel, fileCategory, fileIconSpec, formatSize, formatRelativeTime } from "../viewer/src/presentation/repoTreeFormat.js";

test("fileTypeLabel maps known extensions and falls back to uppercase", () => {
  assert.equal(fileTypeLabel("admin.ts"), "TS");
  assert.equal(fileTypeLabel("data.json"), "JSON");
  assert.equal(fileTypeLabel("README.md"), "MD");
  assert.equal(fileTypeLabel("styles.scss"), "SCSS");
  assert.equal(fileTypeLabel("weird.xyz"), "XYZ");
  assert.equal(fileTypeLabel("a.verylongext"), "VERY"); // capped at 4
});

test("fileTypeLabel treats dotfiles and extensionless names as having no label", () => {
  assert.equal(fileTypeLabel(".gitignore"), "");
  assert.equal(fileTypeLabel("LICENSE"), "");
  assert.equal(fileTypeLabel("Dockerfile"), "");
});

test("fileCategory groups by technology family", () => {
  assert.equal(fileCategory("a.ts"), "code");
  assert.equal(fileCategory("a.json"), "data");
  assert.equal(fileCategory("a.md"), "doc");
  assert.equal(fileCategory("a.css"), "style");
  assert.equal(fileCategory("a.svg"), "asset");
  assert.equal(fileCategory("LICENSE"), "other");
});

test("fileIconSpec resolves brand logos, tinted glyphs, specials, and generic", () => {
  assert.deepEqual(fileIconSpec("admin.ts"), { kind: "brand", key: "typescript" });
  assert.deepEqual(fileIconSpec("App.tsx"), { kind: "brand", key: "react" });
  assert.deepEqual(fileIconSpec("server.py"), { kind: "brand", key: "python" });
  // monochrome-on-dark types fall back to a tinted glyph, not a brand logo
  assert.equal(fileIconSpec("data.json").kind, "glyph");
  assert.equal(fileIconSpec("data.json").icon, "braces");
  assert.equal(fileIconSpec("README.md").icon, "markdown");
  assert.equal(fileIconSpec("config.yml").icon, "hash");
  // special filenames + generic fallback
  assert.deepEqual(fileIconSpec("Dockerfile"), { kind: "brand", key: "docker" });
  assert.equal(fileIconSpec("LICENSE").icon, "file");
  // every glyph spec carries a tint color
  assert.match(fileIconSpec("x.svg").color, /^#/);
});

test("formatSize renders bytes/KB/MB and blanks null", () => {
  assert.equal(formatSize(0), "0 B");
  assert.equal(formatSize(512), "512 B");
  assert.equal(formatSize(1024), "1.0 KB");
  assert.equal(formatSize(2560), "2.5 KB");
  assert.equal(formatSize(1024 * 1024 * 3.4), "3.4 MB");
  assert.equal(formatSize(null), "");
});

test("formatRelativeTime gives compact buckets and blanks null", () => {
  const now = 1_000_000_000_000;
  assert.equal(formatRelativeTime(now, now), "just now");
  assert.equal(formatRelativeTime(now - 5 * 60 * 1000, now), "5m");
  assert.equal(formatRelativeTime(now - 3 * 60 * 60 * 1000, now), "3h");
  assert.equal(formatRelativeTime(now - 4 * 24 * 60 * 60 * 1000, now), "4d");
  assert.equal(formatRelativeTime(now - 60 * 24 * 60 * 60 * 1000, now), "2mo");
  assert.equal(formatRelativeTime(now - 800 * 24 * 60 * 60 * 1000, now), "2y");
  assert.equal(formatRelativeTime(null, now), "");
});
