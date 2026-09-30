'use strict';
// Only used for the first import into an empty workspace. Later syncs use records.
const fs = require('fs');
const path = require('path');
const { DatabaseSync, backup } = require('node:sqlite');
const create = require(process.argv[4]);
const source = fs.realpathSync.native(process.argv[2]);
const staging = fs.realpathSync.native(process.argv[3]);
const workspace = path.join(staging, 'project', 'workspace');
const databases = JSON.parse(fs.readFileSync(0, 'utf8'));
async function main() {
  // A project can have millions of rows across its databases and backups. Never
  // stringify the combined inventory: it can exceed V8's maximum string length.
  // Each helper page is byte bounded; write it and release it before the next.
  const baseline = path.join(staging, 'database-baselines.json');
  const partial = baseline + '.partial';
  const output = fs.openSync(partial, 'wx', 0o600);
  let relative, phase = 'checkpoint';
  const write = text => fs.writeFileSync(output, text);
  try {
    write('{');
    for (const [index, database] of databases.entries()) {
      relative = database;
      phase = 'snapshot';
      if (!relative || relative.split('/').some(p => !p || p === '.' || p === '..') || relative.includes('\\') || relative.includes(':')) throw Error('Invalid snapshot path');
      const destination = path.join(workspace, relative);
      fs.mkdirSync(path.dirname(destination), { recursive: true });
      const report = progress => process.stdout.write(JSON.stringify({path:relative, index:index+1, total:databases.length, ...progress}) + '\n');
      report({phase:'snapshot'});
      const db = new DatabaseSync(path.join(source, relative), { readOnly:true, timeout:5000, allowExtension:false });
      try {
        // Pin one committed read view, including WAL, without replacing or
        // checkpointing the user's open database. Preserve rowids and metadata.
        db.exec('BEGIN');
        db.prepare('SELECT count(*) FROM sqlite_schema').get();
        await backup(db, destination, {rate:256, progress:({totalPages,remainingPages}) => report({phase:'snapshot', done:totalPages-remainingPages, pages:totalPages})});
        db.exec('COMMIT');
      } finally { db.close(); }
      phase = 'index';
      const helper = create(p => path.join(workspace, p));
      try {
        write((index ? ',' : '') + JSON.stringify(relative) + ':{');
        let offset = 0, first = true;
        for (;;) {
          const reply = helper.request({op:'sqlite-scan', path:relative, offset}, progress => report({phase:'index', done:progress.done, records:progress.total}));
          if (reply.entries.length) {
            write((first ? '' : ',') + reply.entries.map(([key, hash]) => JSON.stringify(key) + ':' + JSON.stringify(hash)).join(','));
            first = false;
          }
          if (reply.next === null) break;
          if (!Number.isSafeInteger(reply.next) || reply.next <= offset) throw Error('Invalid SQLite inventory page');
          offset = reply.next;
        }
        write('}');
      } finally { helper.close(); }
      report({phase:'complete'});
    }
    phase = 'checkpoint';
    write('}');
  } catch (error) {
    throw Error(`${relative || 'Database inventory'} (${phase}): ${error.message}`);
  } finally { fs.closeSync(output); }
  // Only expose the checkpoint after every snapshot and inventory succeeded.
  fs.renameSync(partial, baseline);
}
main().catch(error => { console.error(error.message); process.exitCode = 1; });
