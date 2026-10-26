// Extracts factual data about the AK980 Max from a downloaded copy of the Ajazz web driver
// (https://ajazz.driveall.cn/assets/*.js). The bundle is parsed with acorn and never executed:
// only literal values (numbers, strings, arrays, objects, !0/!1, -n, template strings without
// expressions) are converted; anything else becomes null and is reported.
//
// Usage: node extract-site-data.mjs <assetsDir> <outDir>
// Writes layout.json, model.json, keycodes.json, functions.json, effects.json into <outDir>.
// Exits non-zero when the model or its key layout cannot be extracted cleanly.
import fs from "node:fs";
import path from "node:path";
import { Parser } from "acorn";

const VENDOR_ID = 3141;
const PRODUCT_ID = 32835;
const MODEL_NAME = "AK980 MAX";

// The site's keyboard view (keyboard-*.js + main.css): every key is `key-item w-14 h-14` (56 px)
// plus the key's className; a row is `flex justify-between gap-1` (4 px); rows are 60 px apart
// (`mb-1`, and `mt--15` pulls a row up under the previous row's 2-row-tall keys); the container
// is `w-fit`, so its width is the widest row. One spacing step (w-1, ml-1, ...) is 4 px.
const KEY_PX = 56;
const GAP_PX = 4;
const PITCH_PX = KEY_PX + GAP_PX; // 1u
const STEP_PX = 4;

// DOM `code` values the site spells wrongly in its key list.
const CODE_FIXES = { Numpadsubtract: "NumpadSubtract", "-1": "" };

const [assetsDir, outDir] = process.argv.slice(2);
if (!assetsDir || !outDir) {
  console.error("usage: node extract-site-data.mjs <assetsDir> <outDir>");
  process.exit(2);
}

const problems = []; // fatal
const warnings = [];

function fail(message) {
  console.error(`error: ${message}`);
  process.exit(1);
}

// ---------------------------------------------------------------- AST helpers
function bundleFiles(predicate) {
  return fs.readdirSync(assetsDir).filter((f) => f.endsWith(".js") && predicate(f));
}

function parseFile(file) {
  const source = fs.readFileSync(path.join(assetsDir, file), "utf8");
  return Parser.parse(source, { ecmaVersion: "latest", sourceType: "module" });
}

function walk(node, visit) {
  const stack = [node];
  while (stack.length) {
    const n = stack.pop();
    visit(n);
    for (const key in n) {
      if (key === "type" || key === "start" || key === "end") continue;
      const v = n[key];
      if (Array.isArray(v)) {
        for (const c of v) if (c && typeof c.type === "string") stack.push(c);
      } else if (v && typeof v.type === "string") {
        stack.push(v);
      }
    }
  }
}

// name -> initializer of `var/let/const name = <init>` (minified names are unique at module
// scope; when a name repeats, a literal initializer wins over a non-literal one).
function declarations(ast) {
  const literalish = ["ArrayExpression", "ObjectExpression", "Literal", "TemplateLiteral"];
  const map = new Map();
  walk(ast, (n) => {
    if (n.type !== "VariableDeclarator" || n.id.type !== "Identifier" || !n.init) return;
    const prev = map.get(n.id.name);
    if (!prev || (!literalish.includes(prev.type) && literalish.includes(n.init.type))) map.set(n.id.name, n.init);
  });
  return map;
}

function propKey(p) {
  if (p.key.type === "Identifier") return p.key.name;
  if (p.key.type === "Literal") return String(p.key.value);
  return null;
}

function prop(obj, name) {
  return obj.properties.find((p) => p.type === "Property" && propKey(p) === name);
}

function literalValue(node) {
  if (node.type === "Literal") return node.value;
  if (node.type === "TemplateLiteral" && node.expressions.length === 0) return node.quasis[0].value.cooked;
  return undefined;
}

// Literal AST -> JSON. Identifiers are resolved through `decls` when given; anything else is
// reported into `sink` and becomes null.
function toJson(node, where, decls, sink, depth = 0) {
  const bad = (why) => {
    sink.push(`${where}: ${why}`);
    return null;
  };
  if (depth > 30) return bad("too deep");
  switch (node.type) {
    case "Literal":
      return node.value;
    case "TemplateLiteral":
      return node.expressions.length === 0 ? node.quasis[0].value.cooked : bad("template with expressions");
    case "UnaryExpression":
      if (node.operator === "!" && node.argument.type === "Literal") return !node.argument.value;
      if (node.operator === "-" && node.argument.type === "Literal") return -node.argument.value;
      return bad(`unary ${node.operator}`);
    case "ArrayExpression":
      return node.elements.map((e, i) => (e ? toJson(e, `${where}[${i}]`, decls, sink, depth + 1) : null));
    case "ObjectExpression": {
      const out = {};
      for (const p of node.properties) {
        const k = p.type === "Property" ? propKey(p) : null;
        if (k === null) {
          bad(p.type === "Property" ? "computed key" : "spread");
          continue;
        }
        out[k] = toJson(p.value, `${where}.${k}`, decls, sink, depth + 1);
      }
      return out;
    }
    case "Identifier":
      if (decls && decls.has(node.name)) return toJson(decls.get(node.name), where, decls, sink, depth + 1);
      return bad(`identifier ${node.name}`);
    default:
      return bad(node.type);
  }
}

// ---------------------------------------------------------------- main bundle: model config
const mainFiles = bundleFiles((f) => f.startsWith("layout-classic-"));
if (mainFiles.length !== 1) fail(`expected one layout-classic-*.js in ${assetsDir}, found ${mainFiles.length}`);
const mainFile = mainFiles[0];
const mainAst = parseFile(mainFile);
const mainDecls = declarations(mainAst);

const configNodes = [];
const filterNodes = [];
let charsFacade = null;
walk(mainAst, (n) => {
  if (n.type !== "ObjectExpression") return;
  const vid = prop(n, "vendorId");
  const pid = prop(n, "productId");
  if (vid && pid && literalValue(vid.value) === VENDOR_ID && literalValue(pid.value) === PRODUCT_ID) {
    if (prop(n, "keyList")) configNodes.push(n);
    else if (prop(n, "suppliers")) filterNodes.push(n);
  }
  // t({ allKeyChars: () => $Y, basicChars: () => eX, ... }): the protocol's key-character
  // module. Other objects reuse these property names with non-arrow values; skip them.
  const isFacade = ["basicChars", "extendChars", "specialChars", "funcChars", "funcV2Chars"].every((name) => {
    const p = prop(n, name);
    return p && p.value.type === "ArrowFunctionExpression" && p.value.body.type === "Identifier";
  });
  if (isFacade) charsFacade = n;
});

// Pick by product id (several AK980 MAX configs exist for other PIDs); the name only confirms.
const modelProblems = [];
const configs = configNodes.map((n, i) => toJson(n, `config#${i}`, null, modelProblems));
const named = [...new Map(configs.filter((c) => c.name === MODEL_NAME).map((c) => [JSON.stringify(c), c])).values()];
if (named.length !== 1) {
  fail(`expected one "${MODEL_NAME}" config with productId ${PRODUCT_ID}, found ${named.length} (of ${configs.length})`);
}
const model = named[0];
if (filterNodes.length === 0) problems.push(`no device filter entry for productId ${PRODUCT_ID}`);
problems.push(...modelProblems.map((p) => `model ${p}`));
// Effects below are the protocol's default list; a lightingConfig would filter or extend it.
if (model.lightingConfig) problems.push("model has a lightingConfig: effect filtering is not implemented");

// ---------------------------------------------------------------- layout
function cellFromClass(className, where) {
  const cell = { w: KEY_PX, h: KEY_PX, ml: 0, mr: 0, mlAuto: false, mrAuto: false, grow: false };
  for (const c of (className || "").split(/\s+/).filter(Boolean)) {
    let m;
    if ((m = /^w-(\d+)$/.exec(c))) cell.w = Number(m[1]) * STEP_PX;
    else if ((m = /^h-(\d+)$/.exec(c))) cell.h = Number(m[1]) * STEP_PX;
    else if ((m = /^ml-(\d+)$/.exec(c))) cell.ml = Number(m[1]) * STEP_PX;
    else if ((m = /^mr-(\d+)$/.exec(c))) cell.mr = Number(m[1]) * STEP_PX;
    else if (c === "ml-auto") cell.mlAuto = true;
    else if (c === "mr-auto") cell.mrAuto = true;
    else if (c === "flex-1") cell.grow = true;
    else if (c !== "mt--15") problems.push(`${where}: unknown class ${c}`);
    // mt--15 pulls the row up by one pitch under the previous row's tall keys; rows stay
    // one pitch apart, so the row index alone gives y.
  }
  return cell;
}

// Width a row needs without free space; flex-1 items have flex-basis 0.
const fixedWidth = (cells) => cells.reduce((s, c) => s + (c.grow ? 0 : c.w) + c.ml + c.mr, 0) + GAP_PX * (cells.length - 1);
// Max-content width of a row (what `w-fit` sizes the container to): items at their width.
const contentWidth = (cells) => cells.reduce((s, c) => s + c.w + c.ml + c.mr, 0) + GAP_PX * (cells.length - 1);

const round = (v) => Math.round(v * 1000) / 1000;

const rows = model.keyList;
if (!Array.isArray(rows) || rows.length === 0) fail("model has no keyList");
const rowCells = rows.map((row, r) => row.map((k, i) => cellFromClass(k.className, `keyList[${r}][${i}]`)));
const containerPx = Math.max(...rowCells.map(contentWidth));

const keys = [];
rows.forEach((row, r) => {
  const cells = rowCells[r];
  const free = containerPx - fixedWidth(cells);
  const grows = cells.filter((c) => c.grow).length;
  const autos = cells.reduce((n, c) => n + (c.mlAuto ? 1 : 0) + (c.mrAuto ? 1 : 0), 0);
  // Flexbox: free space goes to flex-grow items first, then auto margins, then justify-between.
  const growPx = grows ? free / grows : 0;
  const autoPx = !grows && autos ? free / autos : 0;
  const betweenPx = !grows && !autos && cells.length > 1 ? free / (cells.length - 1) : 0;
  let x = 0;
  row.forEach((k, i) => {
    const c = cells[i];
    const w = c.grow ? growPx : c.w;
    x += c.ml + (c.mlAuto ? autoPx : 0);
    const code = typeof k.key === "string" && k.key in CODE_FIXES ? CODE_FIXES[k.key] : (k.key ?? "");
    keys.push({
      index: k.value,
      label: typeof k.name === "string" ? k.name.trim() : "",
      hid: k.keyCode ?? 0,
      code,
      x: round(x / PITCH_PX),
      y: r,
      w: round((w + GAP_PX) / PITCH_PX),
      h: round((c.h + GAP_PX) / PITCH_PX),
    });
    x += w + c.mr + (c.mrAuto ? autoPx : 0) + GAP_PX + betweenPx;
  });
});

const seen = new Set();
for (const k of keys) {
  if (!Number.isInteger(k.index) || k.index < 0 || k.index > 127) problems.push(`layout: bad index ${k.index} (${k.label})`);
  if (seen.has(k.index)) problems.push(`layout: duplicate index ${k.index} (${k.label})`);
  seen.add(k.index);
  if (!k.label) problems.push(`layout: key ${k.index} has no label`);
  if (!Number.isInteger(k.hid) || k.hid < 0 || k.hid > 0xffff) problems.push(`layout: bad hid ${k.hid} (${k.label})`);
  for (const f of ["x", "y", "w", "h"]) {
    if (!Number.isFinite(k[f]) || k[f] < 0) problems.push(`layout: ${f} of ${k.label} unresolved`);
  }
}
const EPS = 0.002; // coordinates are rounded to 0.001u
for (let i = 0; i < keys.length; i++) {
  for (let j = i + 1; j < keys.length; j++) {
    const a = keys[i];
    const b = keys[j];
    if (a.x < b.x + b.w - EPS && b.x < a.x + a.w - EPS && a.y < b.y + b.h - EPS && b.y < a.y + a.h - EPS) {
      problems.push(`layout: ${a.label} overlaps ${b.label}`);
    }
  }
}
const esc = keys.find((k) => k.code === "Escape");
if (!esc || esc.w !== 1 || esc.h !== 1) problems.push("layout: Esc is not 1u x 1u");

// ---------------------------------------------------------------- French language pack
const frFiles = bundleFiles((f) => /^fr-[A-Za-z0-9_-]+\.js$/.test(f));
if (frFiles.length !== 1) fail(`expected one fr-*.js language pack, found ${frFiles.length}`);
const frAst = parseFile(frFiles[0]);
const frDecls = declarations(frAst);
let frObject = null;
walk(frAst, (n) => {
  if (n.type === "ObjectExpression" && prop(n, "mouse_text1") && (!frObject || n.properties.length > frObject.properties.length)) {
    frObject = n;
  }
});
if (!frObject) fail("French dictionary not found");
const fr = {};
for (const p of frObject.properties) {
  if (p.type !== "Property") continue;
  const sink = [];
  const v = toJson(p.value, `fr.${propKey(p)}`, frDecls, sink);
  if (typeof v === "string") fr[propKey(p)] = v; // non-literal strings are simply skipped
}

// ---------------------------------------------------------------- key tables
if (!charsFacade) fail("key-character module not found");
const charTables = {};
for (const p of charsFacade.properties) {
  if (p.type !== "Property" || p.value.type !== "ArrowFunctionExpression") continue;
  charTables[propKey(p)] = toJson(p.value.body, `chars.${propKey(p)}`, mainDecls, problems);
}

// `fr`: the French label the site shows — the i18n text when the entry has one, otherwise
// the site's French (AZERTY) legend `frText`.
function frLabel(item) {
  return (item.text && fr[item.text]) || item.frText || undefined;
}

function keyEntry(item) {
  const out = { page: item.page, name: item.name, value: item.value };
  if (item.param1 !== undefined) out.param1 = item.param1;
  const label = frLabel(item);
  if (label) out.fr = label;
  return out;
}

function funcEntry(item) {
  const out = { page: item.page, value: item.value, name: item.name };
  if (item.keyType !== undefined) out.keyType = item.keyType;
  const label = frLabel(item);
  if (label) out.fr = label;
  return out;
}

const keyTableNames = ["basicChars", "extendChars", "specialChars"];
for (const t of [...keyTableNames, "funcChars", "funcV2Chars"]) {
  if (!Array.isArray(charTables[t]) || charTables[t].length === 0) problems.push(`table ${t} missing or empty`);
}
const keycodes = keyTableNames.flatMap((t) => (charTables[t] || []).map(keyEntry));
const functions = [...(charTables.funcChars || []), ...(charTables.funcV2Chars || [])].map(funcEntry);

// ---------------------------------------------------------------- built-in lighting effects
const lightingFiles = bundleFiles(
  (f) => f.startsWith("lighting-") && fs.readFileSync(path.join(assetsDir, f), "utf8").includes("defalutLightingModeList"),
);
if (lightingFiles.length !== 1) fail(`expected one protocol lighting module, found ${lightingFiles.length}`);
const lightingAst = parseFile(lightingFiles[0]);
const lightingDecls = declarations(lightingAst);
const lightingExports = {};
walk(lightingAst, (n) => {
  if (n.type !== "ExportNamedDeclaration") return;
  for (const s of n.specifiers) {
    lightingExports[s.exported.name] = toJson(s.local, `lighting.${s.exported.name}`, lightingDecls, problems);
  }
});
const customMode = lightingExports.CUSTOM_LIGHTING_MODE;
const modeList = lightingExports.defalutLightingModeList;
if (!Number.isInteger(customMode) || !Array.isArray(modeList)) problems.push("lighting module exports not understood");

// Direction buttons of the lighting pane: "left" effects use 1 (←) / 0 (→), "top" ones 2 (↑) / 3 (↓).
const DIRECTIONS = { left: [1, 0], top: [2, 3] };

const effects = [...(modeList || [])]
  .sort((a, b) => (a.sort || 0) - (b.sort || 0))
  .map((e) => {
    const custom = e.value === customMode;
    // The pane offers "monochrome" (0) / "RGB" (1) only for coloured, non-custom effects
    // without a secondary colour; picking a colour always sets color mode 0.
    const choice = e.isShowColor && !custom && !e.isShowSecondaryRGB && !e.isHideColorMode;
    const out = { mode: e.value, name: e.name };
    const label = frLabel(e);
    if (label) out.fr = label;
    out.hasColor = !!e.isShowColor;
    out.colorModes = choice ? [0, 1] : e.isShowColor && !custom ? [0] : [];
    out.hasSpeed = !!e.isShowSpeed;
    if (e.isShowSpeed && e.speedValue !== undefined) out.defaultSpeed = e.speedValue;
    out.hasDirection = !!e.isShowDirection;
    if (e.isShowDirection) {
      out.directions = DIRECTIONS[e.directionPosition] ?? null;
      if (!out.directions) problems.push(`effect ${e.value}: unknown direction position ${e.directionPosition}`);
      out.defaultDirection = e.directionValue ?? 0;
    }
    if (e.isShowSecondaryRGB) out.hasSecondaryColor = true;
    if (custom) out.custom = true;
    return out;
  });

// ---------------------------------------------------------------- model.json
const modelOut = { ...model };
delete modelOut.keyList;
for (const axis of modelOut.performanceConfig?.axisList ?? []) {
  if (fr[axis.name]) axis.fr = fr[axis.name];
  else warnings.push(`axis ${axis.name} has no French name`);
}

// ---------------------------------------------------------------- write
if (problems.length) {
  for (const p of problems) console.error(`problem: ${p}`);
  fail(`${problems.length} problem(s), nothing written`);
}
fs.mkdirSync(outDir, { recursive: true });
const write = (name, data) => fs.writeFileSync(path.join(outDir, name), JSON.stringify(data, null, 2) + "\n");
write("layout.json", { pid: PRODUCT_ID, keys });
write("model.json", modelOut);
write("keycodes.json", keycodes);
write("functions.json", functions);
write("effects.json", effects);

const indices = keys.map((k) => k.index);
console.log(`sources: ${mainFile}, ${lightingFiles[0]}, ${frFiles[0]}`);
console.log(
  `layout: ${keys.length} keys in ${rows.length} rows, indices ${Math.min(...indices)}-${Math.max(...indices)} (distinct), ` +
    `width ${round((containerPx + GAP_PX) / PITCH_PX)}u`,
);
console.log(`keycodes: ${keycodes.length}, functions: ${functions.length}, effects: ${effects.length}`);
for (const w of warnings) console.warn(`warning: ${w}`);
console.log(`wrote 5 files to ${outDir}`);
