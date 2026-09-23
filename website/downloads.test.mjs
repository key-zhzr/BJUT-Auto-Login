import test from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import {detectDevice,readDevice,releaseCatalog,newestCatalog,recommendDownload} from './downloads.js';
import {downloadCards} from './render-downloads.js';
const release=JSON.parse(await readFile(new URL('./release.json',import.meta.url)));
const catalog=releaseCatalog(release);

test('only public, exact installer URLs enter the catalog',()=>{
  assert.equal(catalog.assets.length,release.assets.filter(asset=>/\.(exe|dmg|deb|AppImage|apk)$/.test(asset.name)).length);
  assert.ok(catalog.assets.every(asset=>!asset.name.endsWith('.sig')&&!asset.name.endsWith('.tar.gz')));
  for (const url of ['https://example.com/file.exe','https://github.com/other/repo/releases/download/v0.1.5/file.exe',release.assets[0].browser_download_url+'?redirect=other']) {
    assert.equal(releaseCatalog({...release,assets:[{...release.assets[0],browser_download_url:url}]}),null);
  }
  assert.equal(releaseCatalog({...release,draft:true}),null);
  assert.equal(releaseCatalog({...release,prerelease:true}),null);
});
test('release selection does not downgrade or mistake 0.1.9 for 0.1.10',()=>{
  const atVersion=v=>({...release,tag_name:`v${v}`,assets:release.assets.map(a=>({...a,name:a.name.replaceAll(catalog.version,v),browser_download_url:a.browser_download_url.replaceAll(catalog.version,v)}))});
  assert.equal(newestCatalog([atVersion('0.1.9'),atVersion('0.1.10')],catalog).version,'0.1.10');
  assert.equal(newestCatalog([atVersion('0.1.4')],catalog).version,catalog.version);
  assert.equal(newestCatalog([{...atVersion('9.0.0'),prerelease:true}],catalog).version,catalog.version);
  assert.equal(newestCatalog({},catalog).version,catalog.version);
});
const devices=[
  ['Windows ARM hints override reduced x64 UA',{ua:'Mozilla Windows NT 10.0; Win64; x64',hints:{platform:'Windows',architecture:'arm',bitness:'64'}},'windows','arm64'],
  ['Windows x64',{ua:'Windows NT 10.0; Win64; x64'},'windows','x64'],
  ['Windows WOW64 browser',{hints:{platform:'Windows',architecture:'x86',bitness:'32',wow64:true}},'windows','x64'],
  ['Windows 32-bit',{hints:{platform:'Windows',architecture:'x86',bitness:'32'}},'windows','x86'],
  ['Mac ARM hints override Intel UA',{ua:'Macintosh; Intel Mac OS X 10_15_7',platform:'MacIntel',hints:{platform:'macOS',architecture:'arm',bitness:'64'}},'macos','arm64'],
  ['Safari Intel UA is ambiguous',{ua:'Macintosh; Intel Mac OS X 10_15_7',platform:'MacIntel'},'macos',null],
  ['Mac Apple silicon renderer',{ua:'Macintosh; Intel Mac OS X',renderer:'ANGLE (Apple, ANGLE Metal Renderer: Apple M4 Pro, Unspecified Version)'},'macos','arm64'],
  ['Mac generic renderer is ambiguous',{ua:'Macintosh; Intel Mac OS X',renderer:'Apple GPU'},'macos',null],
  ['Mac Intel renderer',{ua:'Macintosh; Intel Mac OS X',renderer:'Intel Iris Plus Graphics'},'macos','x64'],
  ['iPad desktop mode',{ua:'Macintosh; Intel Mac OS X 10_15_7',platform:'MacIntel',touches:5},'ios',null],
  ['iPhone',{ua:'iPhone CPU iPhone OS 18_0 like Mac OS X'},'ios',null],
  ['Android legacy platform does not prove a 32-bit OS',{ua:'Linux; Android 10; K',platform:'Linux armv8l'},'android',null],
  ['Android native ARM64',{ua:'Linux; Android 10; K',hints:{platform:'Android',architecture:'arm',bitness:'64'}},'android','arm64'],
  ['Android emulator',{ua:'Linux; Android 13; x86_64'},'android','x64'],
  ['Android ambiguous',{ua:'Linux; Android 10; K'},'android',null],
  ['Linux ARM',{ua:'X11; Linux aarch64',platform:'Linux aarch64'},'linux','arm64'],
  ['Linux x64',{ua:'X11; Linux x86_64'},'linux','x64'],
  ['ChromeOS is not native Linux',{ua:'X11; CrOS x86_64 14541'},'chromeos',null],
  ['Unknown',{ua:'unknown'},null,null],
];
for (const [label,input,platform,arch] of devices) test(label,()=>assert.deepEqual({platform:detectDevice(input).platform,arch:detectDevice(input).arch},{platform,arch}));
test('recommendations respect architecture and never choose signatures/offline packages',()=>{
  for (const platform of ['windows','macos','android']) for (const arch of ['arm64','x64']) {
    const {asset}=recommendDownload({platform,arch,certain:true},catalog);
    assert.equal(asset.platform,platform); assert.equal(asset.arch,arch); assert.equal(asset.offline,false);
  }
  for (const device of [{platform:'linux',arch:'arm64'},{platform:'windows',arch:'x86'},{platform:'ios'},{platform:null}]) assert.equal(recommendDownload(device,catalog).asset,null);
  assert.match(recommendDownload({platform:'macos',arch:null,certain:false},catalog).note,/芯片未确认/);
});
test('denied client hints fall back without preventing download',async()=>{
  const device=await readDevice({userAgent:'Windows NT 10.0; Win64; x64',userAgentData:{getHighEntropyValues:async()=>{throw new Error('denied');}}});
  assert.equal(recommendDownload(device,catalog).asset.id,'windows-x64-exe');
});
test('non-resolving client hints have a bounded wait',async()=>{
  const start=Date.now(); const device=await readDevice({userAgent:'Android 10',userAgentData:{platform:'Android',getHighEntropyValues:()=>new Promise(()=>{})}});
  assert.ok(Date.now()-start<1500); assert.equal(recommendDownload(device,catalog).asset.id,'android-arm64-apk');
});
test('static fallback offers every installer as a direct link with architecture',()=>{
  const html=downloadCards(catalog);
  assert.equal((html.match(/class="download-option"/g)||[]).length,catalog.assets.length);
  assert.ok(!html.includes('releases/latest'));
  for (const asset of catalog.assets) assert.ok(html.includes(asset.url));
  assert.match(html,/Apple 芯片/); assert.match(html,/ARM64/); assert.match(html,/离线安装包/);
});
