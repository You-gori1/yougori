import test from 'node:test'
import assert from 'node:assert/strict'
import { createRequire } from 'node:module'
import { DatabaseSync } from 'node:sqlite'
import { mkdtempSync, rmSync, existsSync, writeFileSync, mkdirSync, readFileSync, statSync, createReadStream } from 'node:fs'
import { constants as bufferLimits } from 'node:buffer'
import { tmpdir } from 'node:os'
import { join, toNamespacedPath } from 'node:path'
import { createHash } from 'node:crypto'
import { spawnSync } from 'node:child_process'
import { fileURLToPath } from 'node:url'
const require = createRequire(import.meta.url)
const create = require('../cli/src/launcher/assets/sqlite.cjs')
const digest = row => row === null ? null : createHash('sha256').update(JSON.stringify(row)).digest('hex')
function fixture(t) {
  const root = mkdtempSync(join(tmpdir(), 'yougori-sqlite-'))
  const db = new DatabaseSync(join(root, 'data.db'))
  db.exec('PRAGMA journal_mode=WAL; CREATE TABLE parent (id TEXT PRIMARY KEY, value TEXT); CREATE TABLE child (id TEXT PRIMARY KEY, parent TEXT REFERENCES parent(id)); CREATE VIRTUAL TABLE search USING fts5(value);')
  const helper = create(p => join(root,p))
  t.after(() => { helper.close(); db.close(); rmSync(root,{recursive:true,force:true}) })
  const call = (r,report) => helper.request({ path:'data.db', ...r },report)
  const info = call({op:'sqlite-info'})
  const scan = () => { let offset=0, rows = new Map(); for (;;) { const r=call({op:'sqlite-scan',offset}); r.entries.forEach(([k,v])=>rows.set(k,v)); if(r.next===null) return rows; offset=r.next } }
  const stage = op => {
    const bytes=Buffer.from(JSON.stringify(op))
    for (let i=0;i<bytes.length;i+=17000) call({op:'sqlite-stage',data:bytes.subarray(i,i+17000).toString('base64'),done:i+17000>=bytes.length})
  }
  return {root,db,call,scan,stage,info}
}
const key=(table,...values)=>JSON.stringify([table,values])

test('snapshot inventories larger than the JavaScript string limit stream with a small heap',async t=>{
  const {root}=fixture(t)
  const empty=new DatabaseSync(join(root,'empty.db'))
  empty.close()
  const staging=mkdtempSync(join(tmpdir(),'yougori-large-checkpoint-'))
  mkdirSync(join(staging,'project','workspace'),{recursive:true})
  t.after(()=>rmSync(staging,{recursive:true,force:true}))
  // Exercise the snapshot writer with the same bounded page contract, without
  // needing to create and copy a second half-gigabyte SQLite fixture as well.
  const prefix='x'.repeat(1400),hash='a'.repeat(64)
  const sample=key('table','00000000:'+prefix)
  const entryBytes=Buffer.byteLength(JSON.stringify(sample)+':'+JSON.stringify(hash)+',')
  const rowsPerDatabase=Math.ceil(bufferLimits.MAX_STRING_LENGTH/entryBytes/2)+64
  const helper=join(staging,'inventory.cjs')
  writeFileSync(helper,`module.exports=()=>({close(){},request(r){
    const total=${rowsPerDatabase},entries=[];
    const end=Math.min(total,r.offset+64);
    for(let i=r.offset;i<end;i++)entries.push([JSON.stringify(['table',[String(i).padStart(8,'0')+':'+${JSON.stringify(prefix)}]]),${JSON.stringify(hash)}]);
    return {entries,total,next:end<total?end:null};
  }});`)
  const script=fileURLToPath(new URL('../cli/src/launcher/assets/sqlite-snapshot.cjs',import.meta.url))
  const result=spawnSync(process.execPath,['--no-warnings','--max-old-space-size=64',script,root,staging,helper],{
    input:'["data.db","empty.db"]',encoding:'utf8',windowsHide:true,timeout:120000,
  })
  assert.equal(result.status,0,result.stderr)
  const checkpoint=join(staging,'database-baselines.json')
  assert.ok(statSync(checkpoint).size>bufferLimits.MAX_STRING_LENGTH)
  assert.equal(existsSync(checkpoint+'.partial'),false)
  // Count every checksum across chunk boundaries without reading the complete
  // JSON into a string (that is precisely the failure this test reproduces).
  const marker=':'+JSON.stringify(hash)
  let tail='',records=0,beginning='',ending=''
  for await(const chunk of createReadStream(checkpoint,{encoding:'utf8'})) {
    const text=tail+chunk
    if(!beginning)beginning=text.slice(0,80)
    let offset=0,index
    while((index=text.indexOf(marker,offset))>=0){records++;offset=index+marker.length}
    tail=text.slice(Math.max(offset,text.length-marker.length+1))
    ending=text.slice(-128)
  }
  assert.equal(records,rowsPerDatabase*2)
  assert.ok(beginning.startsWith('{"data.db":{'))
  assert.ok(ending.endsWith('}}'))
  assert.equal(result.stdout.trim().split('\n').map(JSON.parse).filter(p=>p.phase==='complete').length,2)
})

test('snapshot failures identify the database and phase without exposing an incomplete checkpoint',t=>{
  const {root}=fixture(t)
  const staging=mkdtempSync(join(tmpdir(),'yougori-failed-checkpoint-'))
  mkdirSync(join(staging,'project','workspace'),{recursive:true})
  t.after(()=>rmSync(staging,{recursive:true,force:true}))
  const script=fileURLToPath(new URL('../cli/src/launcher/assets/sqlite-snapshot.cjs',import.meta.url))
  const helper=fileURLToPath(new URL('../cli/src/launcher/assets/sqlite.cjs',import.meta.url))
  const result=spawnSync(process.execPath,['--no-warnings',script,root,staging,helper],{
    input:'["data.db","missing.db"]',encoding:'utf8',windowsHide:true,
  })
  assert.equal(result.status,1)
  assert.match(result.stderr,/missing\.db \(snapshot\):/)
  assert.equal(existsSync(join(staging,'database-baselines.json')),false)
  assert.equal(existsSync(join(root,'missing.db')),false)
})
test('first import snapshots committed WAL data and saves checksums without replacing the open source',t=>{
  const {root,db}=fixture(t)
  const staging=mkdtempSync(join(tmpdir(),'yougori-snapshot with spaces-'))
  mkdirSync(join(staging,'project','workspace'),{recursive:true})
  t.after(()=>rmSync(staging,{recursive:true,force:true}))
  db.exec("CREATE TABLE implicit(value TEXT); INSERT INTO implicit(rowid,value) VALUES (42,'first'),(1000,'second'); PRAGMA user_version=77; PRAGMA application_id=1234; INSERT INTO parent VALUES ('committed','from WAL'); INSERT INTO child VALUES ('child','committed'); INSERT INTO search(value) VALUES ('find me'); BEGIN IMMEDIATE; INSERT INTO parent VALUES ('uncommitted','must stay local')")
  const original=statSync(join(root,'data.db'))
  const script=fileURLToPath(new URL('../cli/src/launcher/assets/sqlite-snapshot.cjs',import.meta.url))
  const helper=fileURLToPath(new URL('../cli/src/launcher/assets/sqlite.cjs',import.meta.url))
  const result=spawnSync(process.execPath,['--no-warnings',script,toNamespacedPath(root),toNamespacedPath(staging),helper],{input:'["data.db"]',encoding:'utf8',windowsHide:true})
  db.exec('ROLLBACK')
  assert.equal(result.status,0,result.stderr)
  const snapshot=new DatabaseSync(join(staging,'project','workspace','data.db'),{readOnly:true})
  try {
    assert.equal(snapshot.prepare('PRAGMA integrity_check').get().integrity_check,'ok')
    assert.deepEqual(snapshot.prepare('SELECT id,value FROM parent').all().map(r=>({...r})),[{id:'committed',value:'from WAL'}])
    assert.equal(snapshot.prepare("SELECT count(*) AS n FROM search WHERE search MATCH 'find'").get().n,1)
    assert.equal(snapshot.prepare('PRAGMA foreign_key_check').all().length,0)
    assert.deepEqual(snapshot.prepare('SELECT rowid,value FROM implicit').all().map(r=>({...r})),[{rowid:42,value:'first'},{rowid:1000,value:'second'}])
    assert.equal(snapshot.prepare('PRAGMA user_version').get().user_version,77)
    assert.equal(snapshot.prepare('PRAGMA application_id').get().application_id,1234)
    const baseline=JSON.parse(readFileSync(join(staging,'database-baselines.json'),'utf8'))['data.db']
    assert.equal(baseline[key('parent','committed')],digest(['committed','from WAL']))
    assert.equal(baseline[key('parent','uncommitted')],undefined)
    assert.equal(statSync(join(root,'data.db')).ino,original.ino)
    assert.notEqual(statSync(join(staging,'project','workspace','data.db')).ino,original.ino)
    const progress=result.stdout.trim().split('\n').map(JSON.parse)
    assert.ok(progress.some(p=>p.phase==='index' && p.done===p.records))
    assert.equal(progress.at(-1).phase,'complete')
  } finally { snapshot.close() }
  db.exec("INSERT INTO parent VALUES ('after','still writable')")
})
test('standalone helper accepts Windows canonical project roots and preserves the JSON protocol',t=>{
  const {root}=fixture(t)
  const script=fileURLToPath(new URL('../cli/src/launcher/assets/sqlite.cjs',import.meta.url))
  const roots = process.platform === 'win32' ? [root,toNamespacedPath(root)] : [root]
  for(const folder of roots) {
    const result=spawnSync(process.execPath,['--no-warnings',script,folder],{
      input:JSON.stringify({op:'sqlite-info',path:'data.db'})+'\n'+JSON.stringify({op:'sqlite-detect',path:'missing.db'})+'\n',
      encoding:'utf8',windowsHide:true,
    })
    assert.equal(result.status,0,result.stderr)
    const replies=result.stdout.trim().split('\n').map(line=>JSON.parse(line))
    assert.ok(replies[0].result.schema.some(table=>table.name==='parent'))
    assert.equal(replies[1].result.database,false)
  }
})
test('WAL commits from an open app connection are visible; unchanged scans and shadow tables are safe',t=>{
  const {db,scan,info}=fixture(t)
  assert.deepEqual([...scan()],[])
  db.exec("INSERT INTO parent VALUES ('a','first'); INSERT INTO search(value) VALUES ('find me')")
  const rows=scan()
  assert.equal(rows.get(key('parent','a')),digest(['a','first']))
  assert.equal(rows.size,2)
  assert.deepEqual(scan(),rows)
  assert.ok(info.schema.every(s=>!s.name.startsWith('search_')))
  db.exec("UPDATE parent SET value='second' WHERE id='a'")
  assert.equal(scan().get(key('parent','a')),digest(['a','second']))
})
test('record transactions preserve app handles, defer foreign keys, and transfer FTS through logical rows',t=>{
  const {db,stage,call}=fixture(t)
  stage({key:key('child','c'),expected:null,row:['c','p']})
  stage({key:key('parent','p'),expected:null,row:['p','value']})
  stage({key:key('search',{integer:'10'}),expected:null,row:[{integer:'10'},'find this']})
  assert.equal(call({op:'sqlite-apply'}).count,3)
  assert.equal(db.prepare("SELECT value FROM parent WHERE id='p'").get().value,'value')
  assert.equal(db.prepare("SELECT value FROM search WHERE search MATCH 'find'").get().value,'find this')
  stage({key:key('parent','p'),expected:digest(['p','value']),row:null})
  stage({key:key('child','c'),expected:digest(['c','p']),row:null})
  assert.equal(call({op:'sqlite-apply'}).count,2)
  assert.equal(db.prepare('SELECT count(*) AS n FROM parent').get().n,0)
})
test('concurrent destination writes roll back the whole batch, and retry is idempotent',t=>{
  const {db,stage,call}=fixture(t)
  db.exec("INSERT INTO parent VALUES ('a','old')")
  stage({key:key('parent','new'),expected:null,row:['new','added']})
  stage({key:key('parent','a'),expected:digest(['a','old']),row:['a','incoming']})
  db.exec("UPDATE parent SET value='newer' WHERE id='a'")
  assert.throws(()=>call({op:'sqlite-apply'}),/destination record changed/)
  assert.equal(db.prepare("SELECT count(*) n FROM parent WHERE id='new'").get().n,0)
  assert.equal(db.prepare("SELECT value FROM parent WHERE id='a'").get().value,'newer')
  stage({key:key('parent','a'),expected:digest(['a','newer']),row:['a','incoming']})
  assert.equal(call({op:'sqlite-apply'}).count,1)
  stage({key:key('parent','a'),expected:digest(['a','newer']),row:['a','incoming']})
  assert.equal(call({op:'sqlite-apply'}).count,0)
})
test('large Unicode rows, 64-bit integers, blobs and composite keys preserve exact values',t=>{
  const {db,stage,call,scan}=fixture(t)
  db.exec('CREATE TABLE typed (a INTEGER, b TEXT, content BLOB, PRIMARY KEY(a,b)) WITHOUT ROWID')
  call({op:'sqlite-info'})
  const row=[{integer:'9223372036854775807'},'🙂'.repeat(18000),{blob:Buffer.alloc(90000,173).toString('base64')}]
  const k=key('typed',row[0],row[1])
  stage({key:k,expected:null,row})
  call({op:'sqlite-apply'})
  assert.equal(scan().get(k),digest(row))
  let chunks=[],offset=0
  for(;;) { const r=call({op:'sqlite-row',key:k,expected:digest(row),offset}); const b=Buffer.from(r.data,'base64');chunks.push(b);offset+=b.length;if(r.done)break }
  assert.deepEqual(JSON.parse(Buffer.concat(chunks)),row)
})
test('new databases copy schema through SQLite and reject source edits or invalid constraints',t=>{
  const {root,db,stage,call,info}=fixture(t)
  const cloned=call({op:'sqlite-create',path:'new.db',schema:info.schema})
  assert.equal(cloned.signature,info.signature)
  assert.ok(existsSync(join(root,'new.db')))
  stage({key:key('child','bad'),expected:null,row:['bad','missing-parent']})
  assert.throws(()=>call({op:'sqlite-apply'}),/FOREIGN KEY/)
  db.exec("INSERT INTO parent VALUES ('a','original')")
  assert.throws(()=>call({op:'sqlite-row',key:key('parent','a'),expected:digest(['a','old']),offset:0}),/source record changed/)
  writeFileSync(join(root,'fake.db'),'not sqlite')
  assert.equal(call({op:'sqlite-detect',path:'fake.db'}).database,false)
})

test('batched records and byte-bounded inventory reduce exchanges and stream real progress',t=>{
  const src=fixture(t),dst=fixture(t)
  src.db.exec('BEGIN')
  const insert=src.db.prepare('INSERT INTO parent VALUES (?,?)')
  for(let i=0;i<1200;i++)insert.run(`id-${i}`,`value-${i}`)
  insert.run('large','🙂'.repeat(40000))
  src.db.exec('COMMIT')
  const progress=[],rows=[]
  let offset=0,pages=0
  for(;;) {
    const page=src.call({op:'sqlite-scan',offset},p=>progress.push(p))
    assert.equal(page.total,1201)
    assert.ok(Buffer.byteLength(JSON.stringify(page))<160*1024)
    rows.push(...page.entries);pages++
    if(page.next===null)break
    offset=page.next
  }
  assert.equal(rows.length,1201)
  assert.ok(pages<=3)
  assert.deepEqual(progress[0],{phase:'index',done:0,total:1201})
  assert.deepEqual(progress.at(-1),{phase:'index',done:1201,total:1201})
  let exports=0,stages=0,large=0
  for(let start=0;start<rows.length;start+=64) {
    const entries=rows.slice(start,start+64).map(([key,hash])=>[key,hash,null])
    let offset=0
    while(offset<entries.length) {
      const r=src.call({op:'sqlite-export',entries,offset});exports++
      assert.ok(Buffer.byteLength(JSON.stringify(r.operations))<=96*1024+128)
      if(r.operations.length) {dst.call({op:'sqlite-stage-batch',operations:r.operations});stages++}
      offset=r.next
      if(r.large) {
        const [key,expected]=entries[offset],data=[]
        let position=0
        for(;;) {const r=src.call({op:'sqlite-row',key,expected,offset:position});const chunk=Buffer.from(r.data,'base64');data.push(chunk);position+=chunk.length;if(r.done)break}
        dst.stage({key,row:JSON.parse(Buffer.concat(data)),expected:null});offset++;large++
      }
    }
  }
  assert.equal(large,1)
  assert.ok(exports<25 && stages<25,`exports=${exports}, stages=${stages}`)
  const commitProgress=[]
  assert.equal(dst.call({op:'sqlite-apply'},p=>commitProgress.push(p)).count,1201)
  assert.deepEqual(commitProgress[0],{phase:'validate',done:0,total:1201})
  assert.deepEqual(commitProgress.at(-1),{phase:'apply',done:1201,total:1201})
  assert.deepEqual(dst.scan(),src.scan())
})

test('batch exports reject edited sources and the whole destination batch rolls back on conflict',t=>{
  const {db,call}=fixture(t)
  db.exec("INSERT INTO parent VALUES ('a','old')")
  const k=key('parent','a')
  assert.throws(()=>call({op:'sqlite-export',entries:[[k,digest(['a','stale']),null]],offset:0}),/source record changed/)
  assert.throws(()=>call({op:'sqlite-export',entries:[[k,null,null]],offset:0}),/source record changed/)
  call({op:'sqlite-stage-batch',operations:[
    {key:key('parent','new'),row:['new','added'],expected:null},
    {key:k,row:['a','incoming'],expected:digest(['a','old'])},
  ]})
  db.exec("UPDATE parent SET value='edited' WHERE id='a'")
  assert.throws(()=>call({op:'sqlite-apply'}),/destination record changed/)
  assert.equal(db.prepare("SELECT count(*) n FROM parent WHERE id='new'").get().n,0)
  assert.equal(db.prepare("SELECT value FROM parent WHERE id='a'").get().value,'edited')
})

test('database revisions detect app writes, our commits, and helper restarts',t=>{
  const {root,db,call,stage}=fixture(t)
  const revision=()=>call({op:'sqlite-info'}).revision
  const before=revision()
  assert.equal(revision(),before)
  db.exec("INSERT INTO parent VALUES ('a','from-app')")
  const external=revision()
  assert.notEqual(external,before)
  stage({key:key('parent','a'),row:['a','from-sync'],expected:digest(['a','from-app'])})
  call({op:'sqlite-apply'})
  const own=revision()
  assert.notEqual(own,external)
  assert.equal(revision(),own)
  call({op:'sqlite-apply'}) // no staged writes must not invalidate an unchanged database
  assert.equal(revision(),own)
  const restarted=create(p=>join(root,p))
  try { assert.notEqual(restarted.request({op:'sqlite-info',path:'data.db'}).revision,own) }
  finally { restarted.close() }
})
