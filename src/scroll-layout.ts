/** Center content in the visible window, regardless of native scrollbar style.
 * Measure the real gutter: overlay scrollbars use zero, classic ones do not. */
export function setupScrollLayout(): void {
  const observed=new WeakSet<Element>();
  const update=(element:HTMLElement)=>{
    const style=getComputedStyle(element);
    const border=(parseFloat(style.borderLeftWidth)||0)+(parseFloat(style.borderRightWidth)||0);
    const gutter=Math.max(0,element.offsetWidth-element.clientWidth-border);
    const value=`${gutter}px`;
    if(element.style.getPropertyValue('--scroll-inset')!==value)element.style.setProperty('--scroll-inset',value);
  };
  const observer=new ResizeObserver(entries=>entries.forEach(entry=>update(entry.target as HTMLElement)));
  const watch=(element:Element)=>{if(!observed.has(element)){observed.add(element);observer.observe(element);update(element as HTMLElement);}};
  document.querySelectorAll('#main-content,.modal-content').forEach(watch);
  new MutationObserver(records=>records.forEach(record=>record.addedNodes.forEach(node=>{
    if(!(node instanceof Element))return;
    if(node.matches('.modal-content'))watch(node);
    node.querySelectorAll('.modal-content').forEach(watch);
  }))).observe(document.body,{childList:true,subtree:true});
}
