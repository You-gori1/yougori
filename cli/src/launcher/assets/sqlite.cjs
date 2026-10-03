'use strict';
// SQLite records travel through SQLite transactions, never by replacing open DB/WAL files.
const fs = require('fs');
const path = require('path');
const crypto = require('crypto');
const quote = s => '"' + s.replaceAll('"', '""') + '"';
const hash = s => crypto.createHash('sha256').update(s).digest('hex');
const encode = x => typeof x === 'bigint' ? { integer: String(x) } : x instanceof Uint8Array ? { blob: Buffer.from(x).toString('base64') } : x;
const decode = x => x && typeof x === 'object' ? x.integer !== undefined ? BigInt(x.integer) : Buffer.from(x.blob, 'base64') : x;
function isDatabase(file) {
  let fd;
  try { fd = fs.openSync(file, 'r'); const b = Buffer.alloc(16); return fs.readSync(fd, b, 0, 16, 0) === 16 && b.toString('binary') === 'SQLite format 3\0'; }
  catch (e) { if (e.code === 'ENOENT' || e.code === 'EISDIR') return false; throw e; }
  finally { if (fd !== undefined) fs.closeSync(fd); }
}
function* lines(fd) {
  const buffer = Buffer.alloc(64 * 1024);
  const decoder = new (require('string_decoder').StringDecoder)('utf8');
  let rest = '', n, position = 0;
  while ((n = fs.readSync(fd, buffer, 0, buffer.length, position))) {
    position += n;
    rest += decoder.write(buffer.subarray(0, n));
    let end; while ((end = rest.indexOf('\n')) >= 0) { yield JSON.parse(rest.slice(0, end)); rest = rest.slice(end + 1); }
  }
  if (rest + decoder.end()) throw Error('Incomplete SQLite sync batch');
}
module.exports = function create(target) {
  const connections = new Map();
  let staged, rowBuffer;
  function validateStage() {
    target(staged.path);
    const parent = fs.lstatSync(path.dirname(staged.file), { bigint: true });
    const file = fs.lstatSync(staged.file, { bigint: true });
    if (!parent.isDirectory() || parent.dev !== staged.parent.dev || parent.ino !== staged.parent.ino ||
        !file.isFile() || file.dev !== staged.identity.dev || file.ino !== staged.identity.ino) {
      throw Error('SQLite staging location changed; both versions were kept');
    }
  }
  function abort() {
    if (staged) {
      // Recheck the database's parent before removing its staging file. A
      // replaced directory must never redirect cancellation into another tree.
      try { fs.closeSync(staged.fd); } catch {}
      try { validateStage(); fs.unlinkSync(staged.file); } catch {}
      staged = undefined;
    }
  }
  function open(relative, schema) {
    const file = target(relative);
    if (connections.has(file)) {
      const cached = connections.get(file);
      const stat = fs.existsSync(file) ? fs.statSync(file) : null;
      if (stat && cached.ino === stat.ino) return cached;
      cached.db.close(); connections.delete(file);
    }
    if (!fs.existsSync(file) && !schema) return null;
    if (fs.existsSync(file) && !isDatabase(file)) throw Error('Not a SQLite database');
    let DatabaseSync;
    try { ({ DatabaseSync } = require('node:sqlite')); } catch { throw Error('SQLite record sync needs Node.js 22.16 or newer on both sides'); }
    fs.mkdirSync(path.dirname(file), { recursive: true });
    const db = new DatabaseSync(file, { timeout: 5000, allowExtension: false });
    db.exec('PRAGMA foreign_keys=ON');
    try {
      if (schema) {
        db.exec('BEGIN IMMEDIATE');
        try { for (const item of schema) db.exec(item.sql); db.exec('COMMIT'); }
        catch (e) { db.exec('ROLLBACK'); throw e; }
      }
      const c = { db, file, ino: fs.statSync(file).ino, tables: new Map(), statements: new Map(), instance: crypto.randomBytes(12).toString('hex'), generation: 0 };
      connections.set(file, c); return c;
    } catch (e) { db.close(); throw e; }
  }
  function describe(c) {
    const list = c.db.prepare('PRAGMA table_list').all().filter(t => t.schema === 'main');
    const shadow = new Set(list.filter(t => t.type === 'shadow').map(t => t.name));
    const schema = c.db.prepare("SELECT type,name,tbl_name,sql FROM sqlite_schema WHERE sql IS NOT NULL AND name NOT LIKE 'sqlite_%' ORDER BY CASE type WHEN 'table' THEN 0 WHEN 'index' THEN 1 ELSE 2 END,name").all().filter(t => !shadow.has(t.name) && !shadow.has(t.tbl_name));
    const signature = hash(JSON.stringify(schema));
    if (c.schemaSignature !== signature) { c.statements.clear(); c.schemaSignature = signature; }
    c.tables.clear();
    for (const t of list.filter(t => ['table', 'virtual'].includes(t.type) && !t.name.startsWith('sqlite_'))) {
      const columns = c.db.prepare(`PRAGMA table_xinfo(${quote(t.name)})`).all().filter(v => v.hidden === 0);
      const primary = columns.filter(v => v.pk).sort((a,b) => a.pk-b.pk).map(v => v.name);
      const rowid = primary.length === 0;
      if (rowid && columns.some(v => ['rowid','_rowid_','oid'].includes(v.name.toLowerCase()))) throw Error(`No usable primary key in table ${t.name}`);
      c.tables.set(t.name, { columns: columns.map(v => v.name), keys: rowid ? ['rowid'] : primary, rowid });
    }
    // data_version observes writes by the app; generation also observes our own commits.
    const version = c.db.prepare('PRAGMA data_version').get().data_version;
    return { schema, signature, revision: `${c.instance}:${version}:${c.generation}` };
  }
  function statement(c, sql) {
    if (!c.statements.has(sql)) { const s = c.db.prepare(sql); s.setReadBigInts(true); c.statements.set(sql, s); }
    return c.statements.get(sql);
  }
  function unpack(c, key) {
    const [name, values] = JSON.parse(key), table = c.tables.get(name);
    if (!table || !Array.isArray(values) || values.length !== table.keys.length) throw Error('Invalid SQLite row key');
    return { name, table, values: values.map(decode), where: table.keys.map(k => `${quote(k)} IS ?`).join(' AND ') };
  }
  function row(c, key) {
    const {name,table,values,where} = unpack(c,key);
    const cols = (table.rowid ? ['rowid', ...table.columns] : table.columns);
    const value = statement(c, `SELECT ${cols.map(quote).join(',')} FROM ${quote(name)} WHERE ${where}`).get(...values);
    return value ? cols.map(k => encode(value[k])) : null;
  }
  function fingerprint(value) { return value === null ? null : hash(JSON.stringify(value)); }
  function batch(c, relative) {
    if (!staged) {
      const file = path.join(path.dirname(c.file), `.yougori-sync-sqlite-${crypto.randomBytes(12).toString('hex')}`);
      const signature = describe(c).signature;
      const parent = fs.lstatSync(path.dirname(c.file), { bigint: true });
      const fd = fs.openSync(file, 'wx+', 0o600);
      staged = { file, fd, parent, identity: fs.fstatSync(fd, { bigint: true }), path: relative, signature, partial: '', count: 0 };
    }
    if (staged.path !== relative) throw Error('A different SQLite batch is pending');
    try { validateStage(); } catch (error) { abort(); throw error; }
    return staged;
  }
  function request(r, report = () => {}) {
    let last = 0;
    const progress = (phase, done, total, force = false) => {
      const now = Date.now();
      if (force || now - last >= 150) { report({phase, done, total}); last = now; }
    };
    if (r.op === 'sqlite-detect') {
      const file = target(r.path);
      // On POSIX, closing any descriptor for a file releases this process's
      // advisory locks on it. Never header-probe our open DB or its WAL/SHM:
      // another app could then unlink the WAL while we still write through it.
      for (const suffix of ['-wal','-shm','-journal']) {
        if (file.endsWith(suffix) && connections.has(file.slice(0,-suffix.length))) return {database:false};
      }
      const cached = connections.get(file);
      if (cached) {
        const stat = fs.existsSync(file) ? fs.statSync(file) : null;
        if (stat && stat.ino === cached.ino) return {database:true};
        cached.db.close(); connections.delete(file);
      }
      return {database:isDatabase(file)};
    }
    if (r.op === 'sqlite-abort') { abort(); return {}; }
    const c = open(r.path, r.op === 'sqlite-create' ? r.schema : null);
    if (!c) return { missing: true };
    if (r.op === 'sqlite-info' || r.op === 'sqlite-create') return describe(c);
    if (!c.tables.size) describe(c);
    switch (r.op) {
      case 'sqlite-scan': {
        // data_version changes even when SQLite keeps its WAL handle open on Windows.
        const version = c.db.prepare('PRAGMA data_version').get().data_version;
        const signature = describe(c).signature;
        if (!c.inventory || (!r.offset && (c.version !== version || c.signature !== signature))) {
          const inventory = [];
          c.db.exec('BEGIN');
          try {
            let total = 0;
            for (const name of c.tables.keys()) total += Number(statement(c, `SELECT count(*) AS n FROM ${quote(name)}`).get().n);
            progress('index', 0, total, true);
            for (const [name, table] of c.tables) {
              const cols = table.rowid ? ['rowid', ...table.columns] : table.columns;
              for (const value of statement(c, `SELECT ${cols.map(quote).join(',')} FROM ${quote(name)}`).iterate()) {
                const keys = table.keys.map(k => encode(value[k]));
                if (keys.some(k => k === null)) throw Error(`Null primary key in table ${name}`);
                const key = JSON.stringify([name, keys]);
                inventory.push([key, fingerprint(cols.map(k => encode(value[k])))]);
                progress('index', inventory.length, total);
              }
            }
            c.db.exec('COMMIT');
            progress('index', inventory.length, total, true);
          } catch(e) { c.db.exec('ROLLBACK'); throw e; }
          c.inventory = inventory; c.version = version; c.signature = signature;
        }
        const offset = r.offset || 0, entries = [];
        let bytes = 0;
        for (const entry of c.inventory.slice(offset, offset + 512)) {
          const size = Buffer.byteLength(JSON.stringify(entry));
          if (entries.length && bytes + size > 96 * 1024) break;
          if (size > 160 * 1024) throw Error('SQLite row key exceeds the sync reply limit');
          entries.push(entry); bytes += size;
        }
        return { entries, total: c.inventory.length, next: offset + entries.length < c.inventory.length ? offset + entries.length : null };
      }
      case 'sqlite-export': {
        if (!Array.isArray(r.entries) || r.entries.length > 64) throw Error('Invalid SQLite export batch');
        const operations = [];
        let bytes = 0, next = r.offset || 0;
        for (; next < r.entries.length; next++) {
          const [key, expected, destination] = r.entries[next];
          const value = row(c, key);
          if (fingerprint(value) !== expected) throw Error('SQLite source record changed; retrying');
          const op = { key, row: value, expected: destination };
          const size = Buffer.byteLength(JSON.stringify(op)) + 1;
          if (size > 96 * 1024) return { operations, next, large: key };
          if (bytes + size > 96 * 1024) break;
          operations.push(op); bytes += size;
        }
        return { operations, next };
      }
      case 'sqlite-row': {
        if (!r.offset) {
          const value = row(c,r.key);
          if (fingerprint(value) !== r.expected) throw Error('SQLite source record changed; retrying');
          rowBuffer = Buffer.from(JSON.stringify(value));
        }
        if (!rowBuffer) throw Error('Missing SQLite row transfer');
        const data = rowBuffer.subarray(r.offset || 0, (r.offset || 0) + 24 * 1024);
        return { data: data.toString('base64'), total: rowBuffer.length, done: (r.offset || 0) + data.length >= rowBuffer.length };
      }
      case 'sqlite-stage': {
        batch(c, r.path);
        staged.partial += Buffer.from(r.data, 'base64').toString('latin1');
        if (r.done) { const text = Buffer.from(staged.partial, 'latin1').toString('utf8'); JSON.parse(text); fs.appendFileSync(staged.fd, text + '\n'); staged.partial = ''; staged.count++; }
        return {};
      }
      case 'sqlite-stage-batch': {
        if (!Array.isArray(r.operations) || r.operations.length > 64 || Buffer.byteLength(JSON.stringify(r.operations)) > 96 * 1024 + 128) throw Error('Invalid SQLite staging batch');
        for (const op of r.operations) {
          if (!op || typeof op.key !== 'string' || !(op.row === null || Array.isArray(op.row)) || !(op.expected === null || typeof op.expected === 'string')) throw Error('Invalid SQLite staged record');
          unpack(c, op.key);
        }
        batch(c, r.path);
        if (staged.partial) throw Error('A SQLite record transfer is incomplete');
        fs.appendFileSync(staged.fd, r.operations.map(op => JSON.stringify(op) + '\n').join(''));
        staged.count += r.operations.length;
        return {};
      }
      case 'sqlite-apply': {
        if (!staged) return { count: 0 };
        validateStage();
        if (staged.path !== r.path || staged.partial || describe(c).signature !== staged.signature) throw Error('SQLite schema changed during sync');
        c.db.exec('BEGIN IMMEDIATE; PRAGMA defer_foreign_keys=ON');
        let count = 0;
        try {
          let checked = 0, applied = 0;
          progress('validate', 0, staged.count, true);
          // Check every expected row before triggers/cascades from the first write.
          for (const op of lines(staged.fd)) {
            const actual = fingerprint(row(c,op.key));
            if (actual !== op.expected && actual !== fingerprint(op.row)) throw Error('SQLite destination record changed; retrying');
            progress('validate', ++checked, staged.count);
          }
          progress('validate', checked, staged.count, true);
          progress('apply', 0, staged.count, true);
          for (const op of lines(staged.fd)) {
            progress('apply', applied++, staged.count);
            if (fingerprint(row(c,op.key)) === fingerprint(op.row)) continue;
            const { name,table,values,where } = unpack(c,op.key);
            if (op.row === null) statement(c, `DELETE FROM ${quote(name)} WHERE ${where}`).run(...values);
            else {
              const cols = table.rowid ? ['rowid', ...table.columns] : table.columns;
              if (op.row.length !== cols.length) throw Error('SQLite column count changed');
              const vals = op.row.map(decode);
              if (row(c,op.key) !== null) statement(c, `UPDATE ${quote(name)} SET ${cols.map(k=>`${quote(k)}=?`).join(',')} WHERE ${where}`).run(...vals,...values);
              else statement(c, `INSERT INTO ${quote(name)} (${cols.map(quote).join(',')}) VALUES (${cols.map(()=>'?').join(',')})`).run(...vals);
            }
            count++;
          }
          c.db.exec('COMMIT'); c.inventory = undefined; c.generation++;
          progress('apply', applied, staged.count, true);
        } catch(e) { c.db.exec('ROLLBACK'); throw e; }
        finally { abort(); }
        return { count };
      }
      default: throw Error('Unknown SQLite sync operation');
    }
  }
  function close() { abort(); for (const c of connections.values()) c.db.close(); connections.clear(); }
  return { request, close };
};
module.exports.isDatabase = isDatabase;
if (require.main === module) {
  // Rust canonical paths on Windows use \\?\ (including long/UNC paths). The
  // JavaScript resolver fails on these; the native resolver accepts them intact.
  const root = fs.realpathSync.native(process.argv[2]);
  const helper = module.exports(relative => {
    if (!relative || path.isAbsolute(relative) || /[\\\x00-\x1f:]/.test(relative) || relative.split('/').some(p => !p || p === '.' || p === '..')) throw Error('Invalid SQLite path');
    let current = root;
    for (const part of relative.split('/')) {
      current = path.join(current, part);
      try { if (fs.lstatSync(current).isSymbolicLink()) throw Error('SQLite path contains a symlink'); } catch(e) { if(e.code !== 'ENOENT') throw e; }
    }
    return current;
  });
  require('readline').createInterface({input:process.stdin}).on('line', line => {
    try { process.stdout.write(JSON.stringify({result:helper.request(JSON.parse(line), progress => process.stdout.write(JSON.stringify({progress}) + '\n'))}) + '\n'); }
    catch(e) { process.stdout.write(JSON.stringify({error:String(e.message)}) + '\n'); }
  }).on('close', () => helper.close());
}
