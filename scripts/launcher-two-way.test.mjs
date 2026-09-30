import test from 'node:test'
import assert from 'node:assert/strict'
import { createRequire } from 'node:module'
import { mkdtempSync, mkdirSync, writeFileSync, readFileSync, readdirSync, rmSync, existsSync, symlinkSync, statSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { createHash } from 'node:crypto'
import { spawnSync } from 'node:child_process'
import { DatabaseSync } from 'node:sqlite'
const require = createRequire(import.meta.url)
const create = require('../cli/src/launcher/assets/two-way.cjs')
const digest = data => createHash('sha256').update(data).digest('hex')
function fixture(t) {
  const root = mkdtempSync(join(tmpdir(), 'yougori-two-way-'))
  const remote = create(root)
  t.after(() => { remote.abort(); rmSync(root, { recursive: true, force: true }) })
  return { root, call: r => remote.request(r) }
}
test('listing an open WAL database preserves SQLite locks and commits stay visible to fresh app connections', t => {
  const {root,call} = fixture(t)
  const file=join(root,'records.sqlite')
  const db=new DatabaseSync(file)
  db.exec("PRAGMA journal_mode=WAL; CREATE TABLE records(id TEXT PRIMARY KEY,value TEXT); INSERT INTO records VALUES ('base','old')")
  db.close()
  const app = sql => {
    const result=spawnSync(process.execPath,['--no-warnings','-e',"const d=new(require('node:sqlite').DatabaseSync)(process.argv[1]); const q=process.argv[2]; if(q.startsWith('SELECT')) console.log(JSON.stringify(d.prepare(q).all())); else d.exec(q); d.close()",file,sql],{encoding:'utf8',windowsHide:true})
    assert.equal(result.status,0,result.stderr)
    return result.stdout.trim()
  }
  call({op:'sqlite-info',path:'records.sqlite'})
  app("UPDATE records SET value='from app' WHERE id='base'")
  const inventory=call({op:'sqlite-scan',path:'records.sqlite',offset:0})
  assert.ok(existsSync(file+'-wal'))
  // Header probes must not open/close DB or SHM descriptors in the SQL process.
  const listing=call({op:'list',path:'',offset:0})
  assert.equal(listing.entries.find(e=>e.name==='records.sqlite').kind,'sqlite')
  assert.ok(app("SELECT value FROM records WHERE id='base'").includes('from app'))
  assert.ok(existsSync(file+'-wal'),'another app unlinked the sync helper’s open WAL')
  const [key,expected]=inventory.entries[0]
  call({op:'sqlite-stage-batch',path:'records.sqlite',operations:[{key,expected,row:['base','synced commit']}]})
  assert.equal(call({op:'sqlite-apply',path:'records.sqlite'}).count,1)
  assert.ok(app("SELECT value FROM records WHERE id='base'").includes('synced commit'))
})
test('two-way transfers binary files in bounded chunks and preserves the old file until commit', t => {
  const {root, call} = fixture(t)
  const data = Buffer.alloc(70000, 173)
  writeFileSync(join(root, 'file.bin'), 'old')
  call({op:'begin', path:'file.bin', expected:digest('old')})
  for (let offset=0; offset<data.length; offset+=24*1024) call({op:'chunk', data:data.subarray(offset,offset+24*1024).toString('base64')})
  assert.equal(readFileSync(join(root, 'file.bin'), 'utf8'), 'old')
  call({op:'commit', hash:digest(data)})
  const received = []
  for (let offset=0;;) {
    const reply = call({op:'read', path:'file.bin', offset})
    const chunk = Buffer.from(reply.data,'base64')
    assert.ok(chunk.length <= 24*1024)
    received.push(chunk); offset += chunk.length
    if (reply.done) break
  }
  assert.deepEqual(Buffer.concat(received), data)
  assert.deepEqual(call({op:'hashes', paths:['file.bin']}), {'file.bin':digest(data)})
  assert.deepEqual(readdirSync(root), ['file.bin'])
})
test('edits made during upload and delete are kept, and interrupted uploads clean up', t => {
  const {root, call} = fixture(t)
  writeFileSync(join(root,'file.txt'),'base')
  call({op:'begin', path:'file.txt', expected:digest('base')})
  call({op:'chunk', data:Buffer.from('laptop').toString('base64')})
  writeFileSync(join(root,'file.txt'),'terminal edit')
  assert.throws(() => call({op:'commit', hash:digest('laptop')}), /changed/)
  assert.throws(() => call({op:'delete', path:'file.txt', expected:digest('base')}), /changed/)
  assert.equal(readFileSync(join(root,'file.txt'),'utf8'),'terminal edit')
  call({op:'abort'})
  assert.deepEqual(readdirSync(root), ['file.txt'])
  call({op:'delete', path:'file.txt', expected:digest('terminal edit')})
  assert.equal(existsSync(join(root,'file.txt')), false)
})
test('files above 256 MiB upload, hash and read without loading the whole file into memory', t => {
  const { root, call } = fixture(t)
  const chunk = Buffer.alloc(24 * 1024, 173)
  const encoded = chunk.toString('base64')
  const digest = createHash('sha256')
  const count = Math.ceil(257 * 1024 * 1024 / chunk.length)
  writeFileSync(join(root, 'large.db'), 'original')
  const original = call({ op: 'hash', path: 'large.db' }).hash
  call({ op: 'begin', path: 'large.db', expected: original })
  for (let i = 0; i < count; i++) {
    call({ op: 'chunk', data: encoded })
    digest.update(chunk)
  }
  assert.equal(readFileSync(join(root, 'large.db'), 'utf8'), 'original')
  const hash = digest.digest('hex')
  call({ op: 'commit', hash })
  assert.equal(statSync(join(root, 'large.db')).size, count * chunk.length)
  assert.equal(call({ op: 'hash', path: 'large.db' }).hash, hash)
  const last = call({ op: 'read', path: 'large.db', offset: (count - 1) * chunk.length })
  assert.equal(last.done, true)
  assert.deepEqual(Buffer.from(last.data, 'base64'), chunk)
})
test('invalid paths, changed file types, oversized chunks and wrong checksums are rejected', t => {
  const {root, call} = fixture(t)
  for (const path of ['../escape','/absolute','a/../../b','a\\b','a:stream']) assert.throws(() => call({op:'begin',path,expected:null}), /path/)
  mkdirSync(join(root,'folder'))
  assert.throws(() => call({op:'delete',path:'folder',expected:null}), /regular file/)
  call({op:'begin',path:'file',expected:null})
  assert.throws(() => call({op:'chunk',data:Buffer.alloc(24*1024+1).toString('base64')}), /limit/)
  call({op:'chunk',data:Buffer.from('data').toString('base64')})
  assert.throws(() => call({op:'commit',hash:digest('wrong')}), /checksum/)
  call({op:'abort'})
  assert.equal(existsSync(join(root,'file')), false)
})
test('directory listing is paged, new files are discovered, and internal staging files stay hidden', t => {
  const {root,call} = fixture(t)
  for (let i=0; i<150; i++) writeFileSync(join(root,`f${i}`),'')
  writeFileSync(join(root,'.yougori-sync-temp'),'hidden')
  let offset=0, names=[]
  do {
    const page=call({op:'list',path:'',offset})
    assert.ok(page.entries.length <= 64)
    names.push(...page.entries.map(e=>e.name)); offset=page.next
  } while(offset !== null)
  assert.equal(names.length,150)
  assert.equal(new Set(names).size,150)
})
test('directory symlinks and Windows junctions cannot redirect reads or writes', t => {
  const {root,call} = fixture(t)
  const outside = mkdtempSync(join(tmpdir(),'yougori-outside-'))
  t.after(() => rmSync(outside,{recursive:true,force:true}))
  writeFileSync(join(outside,'keep'),'private')
  symlinkSync(outside,join(root,'link'),process.platform === 'win32' ? 'junction' : 'dir')
  assert.throws(() => call({op:'hash',path:'link/keep'}), /Symlink/)
  assert.throws(() => call({op:'begin',path:'link/new',expected:null}), /Symlink/)
  assert.equal(readFileSync(join(outside,'keep'),'utf8'),'private')
})
test('actual sync receiver frames large replies and keeps the one-way protocol working', t => {
  const root = mkdtempSync(join(tmpdir(), 'yougori-sync-protocol-'))
  t.after(()=>rmSync(root,{recursive:true,force:true}))
  const workspace=join(root,'project'), state=join(root,'state'), imported=join(root,'imported')
  mkdirSync(imported); mkdirSync(state)
  // Local launch points /workspace at the imported project. Chunked one-way
  // operations must accept this launcher-owned alias while rejecting child links.
  symlinkSync(imported, workspace, process.platform === 'win32' ? 'junction' : 'dir')
  const asset = new URL('../cli/src/launcher/assets/',import.meta.url)
  const script=readFileSync(new URL('sync.cjs',asset),'utf8')
    .replace("const root = '/workspace';",`const root = ${JSON.stringify(workspace)};`)
    .replace("const state = '/yougori/launch';",`const state = ${JSON.stringify(state)};`)
  writeFileSync(join(root,'sync.cjs'),script)
  writeFileSync(join(root,'two-way.cjs'),readFileSync(new URL('two-way.cjs',asset)))
  writeFileSync(join(root,'sqlite.cjs'),readFileSync(new URL('sqlite.cjs',asset)))
  const data=Buffer.alloc(24000,123)
  const request=Buffer.from(JSON.stringify({op:'read',path:'file',offset:0})).toString('base64')
  const input=`W ${Buffer.from('file').toString('base64')} ${data.toString('base64')}\nB before\nX request ${request}\n`
  const result=spawnSync(process.execPath,[join(root,'sync.cjs'),'test'],{input,encoding:'utf8'})
  assert.equal(result.status,0,result.stderr)
  assert.match(result.stdout,/ack:before/)
  const frames=[...result.stdout.matchAll(/\[yougori-test:rpc:request:([^\]]+)\]/g)].map(m=>m[1])
  assert.ok(frames.length>1)
  assert.ok(frames.every(f=>f.length<=8192))
  const reply=JSON.parse(Buffer.from(frames.join(''),'base64'))
  assert.deepEqual(Buffer.from(reply.result.data,'base64'),data)
  assert.match(result.stdout,/end:request/)
})
