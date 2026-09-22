import { NetworkExperience } from '../src/network-experience';
import { applyAppearance } from '../src/appearance';
const markup = new DOMParser().parseFromString(await (await fetch('/index.html')).text(), 'text/html');
await Promise.all(Array.from(markup.head.querySelectorAll<HTMLLinkElement>('link[rel="stylesheet"]')).map(link => new Promise<void>((resolve,reject) => { link.onload=()=>resolve(); link.onerror=()=>reject(new Error(`样式未加载：${link.href}`)); document.head.append(link); })));
const fixture = document.getElementById('fixture')!;
fixture.append(markup.querySelector('.network-adapter-settings')!);
document.body.style.cssText = 'display:block;overflow:auto;height:auto;min-height:100vh';
const ui = new NetworkExperience(async () => {}, async () => {});
ui.renderInventory({preferredInterface:'',selectionSupported:false,adapters:[
  {id:'101',name:'wlan0',interfaceName:'wlan0',transport:'wifi',connected:true,selected:true,selectable:false,ipv4:['192.168.1.216'],ipv6:['2001:db8::1234']},
  {id:'102',name:'移动数据超长名称012345678901234567890123456789',interfaceName:'wwan1',transport:'cellular',connected:true,selected:false,selectable:false,ipv4:['10.6.86.228','10.6.86.229'],ipv6:['2001:db8::5678']},
  {id:'103',name:'tun0',interfaceName:'tun0',transport:'vpn',connected:true,selected:false,selectable:false,ipv4:['172.19.0.1'],ipv6:[]},
]});
applyAppearance('basic','blue','light');
const tick = () => new Promise<void>(resolve => requestAnimationFrame(() => requestAnimationFrame(() => resolve())));
document.getElementById('run')!.addEventListener('click', async () => {
  const output = document.getElementById('result')!; output.textContent = '';
  try {
    for (const theme of ['basic','apple27','winui'] as const) {
      applyAppearance(theme,'blue','light');
      for (const width of [320,360,390,480]) {
        fixture.style.width = `${width}px`; await tick();
        for (const node of fixture.querySelectorAll<HTMLElement>('.network-adapter-row,.network-adapter-text,.network-adapter-list,.diagnostic-panel-header')) {
          if (node.scrollWidth > node.clientWidth + 1) throw new Error(`${theme} ${width}px 溢出：${node.className} (${node.scrollWidth}/${node.clientWidth})`);
        }
        output.textContent += `通过：${theme} ${width}px 网卡文字完整换行\n`;
      }
    }
    output.dataset.result = 'passed';
  } catch (error) { output.dataset.result='failed'; output.textContent += String(error); }
  finally { applyAppearance('basic','blue','light'); fixture.style.width='390px'; }
});
