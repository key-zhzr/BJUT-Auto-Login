import type { NetworkEvent } from './models';

export function renderNetworkEvents(events: NetworkEvent[]) {
  const list = document.getElementById('network-events-list');
  if (!list) return;
  list.replaceChildren();
  for (const event of events.slice(-12).reverse()) {
    const row = document.createElement('li');
    const time = document.createElement('time'); time.dateTime = event.time;
    time.textContent = new Date(event.time).toLocaleString();
    const message = document.createElement('span'); message.textContent = event.message;
    row.append(time, message); list.append(row);
  }
  if (!list.childElementCount) {
    const empty = document.createElement('li'); empty.className = 'diagnostic-empty';
    empty.textContent = '暂无网络事件，检测和登录后会在此说明原因'; list.append(empty);
  }
}
