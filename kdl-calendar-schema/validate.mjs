// Minimal KDL Schema 1.0.0 validator, enough for calendar.schema.kdl and the
// spec's own meta-schema. Uses kdljs (KDL v2 parser).
//
//   npm i kdljs@0.3.0
//   node validate.mjs calendar.schema.kdl examples/*.kdl
//
// Supported: document/node/value/prop/children, definitions + ref=[id="..."],
// min/max (node counts and value counts), required, other-nodes-allowed,
// other-props-allowed, type, enum, pattern, min-length, max-length,
// > >= < <=, % and format (date-time, date, time, duration, email, url, uuid;
// other formats pass). Not supported: tag/tag-names/node-names/prop-names.
import { parse } from 'kdljs'
import fs from 'node:fs'

const read = (file) => {
  const { output, errors } = parse(fs.readFileSync(file, 'utf8'))
  if (errors.length) throw new Error(`${file}: parse error: ${errors[0].message}`)
  return output
}

export function compile (schemaDoc) {
  const ids = new Map()
  const walk = (n) => {
    if (n.properties.id !== undefined) ids.set(n.properties.id, n)
    n.children.forEach(walk)
  }
  schemaDoc.forEach(walk)

  const keyOf = (n) => `${n.name}\0${n.values[0] ?? ''}`
  // ref: copy the target's values, props and children into this node; on
  // conflict the target's definition wins (SCHEMA-SPEC "replacing any conflicts").
  const resolve = (n, seen = new Set()) => {
    const ref = n.properties.ref
    if (ref === undefined) return n
    const m = /^\[id="([^"]+)"\]$/.exec(ref)
    if (!m) throw new Error(`unsupported ref query: ${ref}`)
    if (seen.has(m[1])) throw new Error(`ref cycle at ${m[1]}`)
    const target = ids.get(m[1])
    if (!target) throw new Error(`ref to unknown id: ${m[1]}`)
    const t = resolve(target, new Set([...seen, m[1]]))
    const { ref: _r, ...ownProps } = n.properties
    const { id: _i, ...targetProps } = t.properties
    const targetKeys = new Set(t.children.map(keyOf))
    return {
      name: n.name,
      values: n.values.length ? n.values : t.values,
      properties: { ...ownProps, ...targetProps },
      children: [...n.children.filter((c) => !targetKeys.has(keyOf(c))), ...t.children],
      tags: n.tags
    }
  }
  const root = schemaDoc.find((n) => n.name === 'document')
  if (!root) throw new Error('schema has no document node')
  return { root, resolve }
}

const one = (spec, name) => spec.children.find((c) => c.name === name)?.values[0]
const all = (spec, name) => spec.children.filter((c) => c.name === name)

function checkFormat (fmt, v) {
  if (typeof v !== 'string') return true
  const re = {
    'date-time': /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}(:\d{2}(\.\d+)?)?(Z|[+-]\d{2}:\d{2})?$/,
    date: /^\d{4}-\d{2}-\d{2}$/,
    time: /^\d{2}:\d{2}(:\d{2}(\.\d+)?)?(Z|[+-]\d{2}:\d{2})?$/,
    duration: /^P(?!$)(\d+Y)?(\d+M)?(\d+W)?(\d+D)?(T(?=\d)(\d+H)?(\d+M)?(\d+(\.\d+)?S)?)?$/,
    email: /^[^@\s]+@[^@\s]+\.[^@\s]+$/,
    uuid: /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i
  }[fmt]
  if (re) {
    if (!re.test(v)) return false
    if (fmt === 'date' || fmt === 'date-time') return !Number.isNaN(Date.parse(v.slice(0, 10)))
    return true
  }
  if (fmt === 'url' || fmt === 'irl') { try { new URL(v); return true } catch { return false } }
  return true
}

function typeOf (v) {
  return v === null ? 'null' : typeof v
}

function checkValue (v, spec, where, errs) {
  const types = all(spec, 'type').flatMap((t) => t.values)
  if (types.length && !types.includes(typeOf(v))) {
    errs.push(`${where}: ${JSON.stringify(v)} is ${typeOf(v)}, expected ${types.join(' or ')}`)
    return
  }
  const en = all(spec, 'enum').flatMap((e) => e.values)
  if (all(spec, 'enum').length && !en.includes(v)) errs.push(`${where}: ${JSON.stringify(v)} is not one of ${en.join(', ')}`)
  if (typeof v === 'string') {
    for (const p of all(spec, 'pattern').flatMap((x) => x.values)) {
      if (!new RegExp(p, 'u').test(v)) errs.push(`${where}: ${JSON.stringify(v)} does not match /${p}/`)
    }
    const len = [...v].length
    const minL = one(spec, 'min-length'); const maxL = one(spec, 'max-length')
    if (minL !== undefined && len < minL) errs.push(`${where}: shorter than ${minL}`)
    if (maxL !== undefined && len > maxL) errs.push(`${where}: longer than ${maxL}`)
  }
  const fmts = all(spec, 'format').flatMap((f) => f.values)
  if (fmts.length && !fmts.some((f) => checkFormat(f, v))) errs.push(`${where}: ${JSON.stringify(v)} is not a valid ${fmts.join(' or ')}`)
  if (typeof v === 'number') {
    const cmp = { '>': (a, b) => a > b, '>=': (a, b) => a >= b, '<': (a, b) => a < b, '<=': (a, b) => a <= b }
    for (const [op, f] of Object.entries(cmp)) {
      const b = one(spec, op)
      if (b !== undefined && !f(v, b)) errs.push(`${where}: ${v} is not ${op} ${b}`)
    }
    for (const m of all(spec, '%').flatMap((x) => x.values)) {
      if (v % m !== 0) errs.push(`${where}: ${v} is not a multiple of ${m}`)
    }
  }
}

function checkNode (node, spec, path, ctx) {
  const { resolve, errs } = ctx
  // values
  const valueSpecs = all(spec, 'value').map((s) => resolve(s))
  if (!valueSpecs.length && node.values.length) errs.push(`${path}: takes no values, got ${node.values.length}`)
  for (const vs of valueSpecs) {
    const min = one(vs, 'min'); const max = one(vs, 'max')
    if (min !== undefined && node.values.length < min) errs.push(`${path}: needs at least ${min} value(s), got ${node.values.length}`)
    if (max !== undefined && node.values.length > max) errs.push(`${path}: allows at most ${max} value(s), got ${node.values.length}`)
    node.values.forEach((v, i) => checkValue(v, vs, `${path} value[${i}]`, errs))
  }
  // props
  const propSpecs = all(spec, 'prop').map((s) => resolve(s))
  const named = new Map(propSpecs.filter((p) => p.values.length).map((p) => [p.values[0], p]))
  const anyProp = propSpecs.filter((p) => !p.values.length)
  const otherProps = one(spec, 'other-props-allowed') === true
  for (const [k, v] of Object.entries(node.properties)) {
    const ps = named.get(k)
    if (ps) checkValue(v, ps, `${path} ${k}=`, errs)
    else if (anyProp.length) anyProp.forEach((p) => checkValue(v, p, `${path} ${k}=`, errs))
    else if (!otherProps) errs.push(`${path}: unknown property ${k}=`)
  }
  for (const [k, ps] of named) {
    if (one(ps, 'required') === true && !(k in node.properties)) errs.push(`${path}: missing required property ${k}=`)
  }
  // children
  const childSpecs = all(spec, 'children').map((s) => resolve(s))
  if (!childSpecs.length) {
    if (node.children.length) errs.push(`${path}: takes no children`)
    return
  }
  checkChildren(node.children, childSpecs, path, ctx)
}

function checkChildren (nodes, blocks, path, ctx) {
  const { resolve, errs } = ctx
  const specs = blocks.flatMap((b) => all(b, 'node')).map((s) => resolve(s))
  const otherNodes = blocks.some((b) => one(b, 'other-nodes-allowed') === true)
  const counts = new Map()
  nodes.forEach((n, i) => {
    const where = `${path}/${n.name}${n.values.length && typeof n.values[0] === 'string' ? `[${JSON.stringify(n.values[0])}]` : `[${i}]`}`
    const matching = specs.filter((s) => !s.values.length || s.values[0] === n.name)
    if (!matching.some((s) => s.values.length) && !otherNodes && !matching.length) {
      errs.push(`${where}: unknown node "${n.name}"`)
      return
    }
    counts.set(n.name, (counts.get(n.name) ?? 0) + 1)
    matching.forEach((s) => checkNode(n, s, where, ctx))
  })
  for (const s of specs.filter((s) => s.values.length)) {
    const name = s.values[0]; const c = counts.get(name) ?? 0
    const min = one(s, 'min'); const max = one(s, 'max')
    if (min !== undefined && c < min) errs.push(`${path || '/'}: needs at least ${min} "${name}" node(s), got ${c}`)
    if (max !== undefined && c > max) errs.push(`${path || '/'}: allows at most ${max} "${name}" node(s), got ${c}`)
  }
}

export function validate (doc, schemaDoc) {
  const { root, resolve } = compile(schemaDoc)
  const ctx = { resolve, errs: [] }
  checkChildren(doc, [root], '', ctx)
  return ctx.errs
}

if (import.meta.url === `file://${process.argv[1]}`) {
  const [schemaFile, ...docs] = process.argv.slice(2)
  if (!schemaFile || !docs.length) {
    console.error('usage: node validate.mjs SCHEMA.kdl DOC.kdl...')
    process.exit(2)
  }
  const schema = read(schemaFile)
  let failed = 0
  for (const f of docs) {
    let errs
    try { errs = validate(read(f), schema) } catch (e) { errs = [e.message] }
    if (errs.length) { failed++; console.log(`✗ ${f}`); errs.forEach((e) => console.log(`    ${e}`)) } else console.log(`✓ ${f}`)
  }
  process.exit(failed ? 1 : 0)
}
