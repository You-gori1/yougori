'use strict';
// Bounded, conditional file operations for opt-in two-way project sync.
const fs = require('fs');
const path = require('path');
const crypto = require('crypto');
// Offsets must remain exactly representable in the file protocol.
const MAX = Number.MAX_SAFE_INTEGER;
module.exports = function create(root) {
  let upload;
  const sqlite = require('./sqlite.cjs')(relative => target(relative));
  const hashes = new Map();
  function target(relative, directory = false, inspectLinks = false) {
    if (relative === '' && directory) return root;
    if (typeof relative !== 'string' || !relative || relative.length > 4096 || /[\\\x00-\x1f:]/.test(relative) || relative.split('/').some(p => !p || p === '.' || p === '..' || p.startsWith('.yougori-sync-'))) throw Error('Invalid sync path');
    let current = root;
    if (fs.lstatSync(root).isSymbolicLink()) throw Error('Workspace is a symlink');
    for (const part of relative.split('/')) {
      current = path.join(current, part);
      try {
        if (fs.lstatSync(current).isSymbolicLink()) {
          if (inspectLinks) return null;
          throw Error(`Symlink blocks sync: ${relative}`);
        }
      }
      catch (e) { if (e.code !== 'ENOENT') throw e; }
    }
    return current;
  }
  function hash(file) {
    let stat;
    try { stat = fs.lstatSync(file); } catch (e) { if (e.code === 'ENOENT') return null; throw e; }
    if (!stat.isFile() || stat.isSymbolicLink()) throw Error('Sync path is not a regular file');
    if (stat.size > MAX) throw Error('File exceeds the supported filesystem offset');
    const fd = fs.openSync(file, fs.constants.O_RDONLY | (fs.constants.O_NOFOLLOW || 0));
    try {
      const digest = crypto.createHash('sha256'), buffer = Buffer.alloc(64 * 1024);
      let n; while ((n = fs.readSync(fd, buffer, 0, buffer.length, null))) digest.update(buffer.subarray(0, n));
      const after = fs.fstatSync(fd);
      if (after.size !== stat.size || after.mtimeMs !== stat.mtimeMs || after.ctimeMs !== stat.ctimeMs || after.ino !== stat.ino) throw Error('File changed during sync; retrying');
      return digest.digest('hex');
    } finally { fs.closeSync(fd); }
  }
  function cachedHash(file) {
    let stat;
    try { stat = fs.lstatSync(file, { bigint: true }); }
    catch (e) { if (e.code === 'ENOENT') { hashes.delete(file); return null; } throw e; }
    const stamp = `${stat.ino}:${stat.size}:${stat.mtimeNs}:${stat.ctimeNs}`;
    const cached = hashes.get(file);
    if (cached && cached.stamp === stamp) return cached.value;
    const value = hash(file);
    if (hashes.size > 100000) hashes.clear();
    hashes.set(file, { stamp, value });
    return value;
  }
  function expected(file, value) {
    if (hash(file) !== value) throw Error('File changed during sync; both versions were kept');
  }
  function validateUpload() {
    target(upload.relative);
    const parent = fs.lstatSync(path.dirname(upload.temp), { bigint: true });
    const staged = fs.lstatSync(upload.temp, { bigint: true });
    if (!parent.isDirectory() || parent.dev !== upload.parent.dev || parent.ino !== upload.parent.ino ||
        !staged.isFile() || staged.dev !== upload.identity.dev || staged.ino !== upload.identity.ino) {
      throw Error('Sync staging location changed; both versions were kept');
    }
  }
  function abort() {
    if (upload) {
      try { fs.closeSync(upload.fd); } catch {}
      // A guest process may replace the parent directory during a transfer.
      // Never let cleanup follow that replacement into unrelated files.
      try { validateUpload(); fs.unlinkSync(upload.temp); } catch {}
      upload = undefined;
    }
  }
  function request(r, report) {
    if (r.op.startsWith('sqlite-')) return sqlite.request(r, report);
    switch (r.op) {
      case 'directories': {
        if (!Array.isArray(r.paths) || r.paths.length > 64) throw Error('Invalid directory batch');
        const errors = [];
        for (const p of r.paths) {
          try { if (r.create) fs.mkdirSync(target(p), { recursive:true }); else fs.rmdirSync(target(p)); }
          catch(e) { if (r.create || e.code !== 'ENOENT') errors.push([p, e.message]); }
        }
        return { errors: Object.fromEntries(errors) };
      }
      case 'mkdir': fs.mkdirSync(target(r.path), { recursive: true }); return {};
      case 'rmdir': {
        try { fs.rmdirSync(target(r.path)); } catch(e) { if (e.code !== 'ENOENT') throw e; }
        return {};
      }
      case 'hashes': {
        if (!Array.isArray(r.paths) || r.paths.length > 64) throw Error('Invalid hash batch');
        if (r.skipLinks === true) {
          const values = [], skipped = [];
          for (const p of r.paths) {
            const file = target(p, false, true);
            if (file === null) skipped.push(p);
            else values.push([p, cachedHash(file)]);
          }
          return { hashes: Object.fromEntries(values), skipped };
        }
        return Object.fromEntries(r.paths.map(p => [p, cachedHash(target(p))]));
      }
      case 'links': {
        if (!Array.isArray(r.paths) || r.paths.length > 64) throw Error('Invalid link batch');
        return { skipped: r.paths.filter(p => target(p, false, true) === null) };
      }
      case 'list': {
        const dir = target(r.path, true);
        const names = fs.readdirSync(dir).filter(n => !n.startsWith('.yougori-sync-')).sort();
        const offset = Number.isSafeInteger(r.offset) && r.offset >= 0 ? r.offset : 0;
        const entries = names.slice(offset, offset + 64).map(name => {
          const s = fs.lstatSync(path.join(dir, name));
          const relative = r.path ? `${r.path}/${name}` : name;
          return { name, kind: s.isSymbolicLink() ? 'link' : s.isDirectory() ? 'dir' : s.isFile() ? (sqlite.request({op:'sqlite-detect',path:relative}).database ? 'sqlite' : 'file') : 'other' };
        });
        return { entries, next: offset + entries.length < names.length ? offset + entries.length : null };
      }
      case 'hash': {
        const file = target(r.path);
        return { hash: hash(file) };
      }
      case 'read': {
        const file = target(r.path);
        if (!Number.isSafeInteger(r.offset) || r.offset < 0 || r.offset > MAX) throw Error('Invalid read offset');
        const fd = fs.openSync(file, fs.constants.O_RDONLY | (fs.constants.O_NOFOLLOW || 0));
        try {
          const s = fs.fstatSync(fd);
          if (!s.isFile() || s.size > MAX) throw Error('Not a supported sync file');
          const data = Buffer.alloc(24 * 1024);
          const n = fs.readSync(fd, data, 0, data.length, r.offset);
          return { data: data.subarray(0, n).toString('base64'), total: s.size, done: r.offset + n >= s.size, mode: s.mode & 0o777 };
        } finally { fs.closeSync(fd); }
      }
      case 'begin': {
        abort();
        const file = target(r.path);
        expected(file, r.expected);
        fs.mkdirSync(path.dirname(file), { recursive: true });
        const temp = path.join(path.dirname(file), `.yougori-sync-${crypto.randomBytes(12).toString('hex')}`);
        let mode = 0o644;
        try { mode = fs.statSync(file).mode & 0o777; } catch {}
        const parent = fs.lstatSync(path.dirname(file), { bigint: true });
        const fd = fs.openSync(temp, 'wx', mode);
        upload = { file, temp, fd, parent, identity: fs.fstatSync(fd, { bigint: true }), relative: r.path, expected: r.expected, bytes: 0 };
        try { validateUpload(); } catch (error) { abort(); throw error; }
        return {};
      }
      case 'chunk': {
        if (!upload) throw Error('No sync upload');
        validateUpload();
        const data = Buffer.from(r.data, 'base64');
        if (data.length > 24 * 1024 || upload.bytes + data.length > MAX) throw Error('Sync upload exceeds limit');
        // Keep the original staging descriptor instead of reopening its name.
        // Directory replacement cannot redirect an in-flight write.
        for (let offset = 0; offset < data.length;) {
          const written = fs.writeSync(upload.fd, data, offset, data.length - offset);
          if (!written) throw Error('Sync upload made no progress');
          offset += written;
        }
        upload.bytes += data.length;
        return {};
      }
      case 'commit': {
        if (!upload) throw Error('No sync upload');
        validateUpload();
        expected(upload.file, upload.expected);
        if (hash(upload.temp) !== r.hash) throw Error('Sync upload checksum mismatch');
        fs.fsyncSync(upload.fd);
        fs.closeSync(upload.fd);
        upload.fd = undefined;
        fs.renameSync(upload.temp, upload.file);
        upload = undefined;
        return {};
      }
      case 'delete': {
        const file = target(r.path);
        expected(file, r.expected);
        if (r.expected !== null) fs.unlinkSync(file);
        return {};
      }
      case 'abort': abort(); return {};
      default: throw Error('Unknown two-way sync operation');
    }
  }
  return { request, abort: () => { abort(); sqlite.close(); } };
};
