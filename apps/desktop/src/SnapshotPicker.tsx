import {useEffect,useId,useState} from 'react';
import {Check,ChevronDown,FolderGit2,Search} from 'lucide-react';
import {listItems,type Item,type Status} from './api';

const sizeLabel=(size:number)=>size<1024?`${size} B`:size<1024**2?`${(size/1024).toFixed(1)} KB`:`${(size/1024**2).toFixed(1)} MB`;

/** Suggestions come from the encrypted catalog, never browser storage. The
 * backend still authenticates and validates the selected Git snapshot. */
export function SnapshotPicker({status,value,onChange,label}:{status:Status;value:string;onChange:(name:string)=>void;label:string}) {
 const id=useId();
 const [open,setOpen]=useState(false),[items,setItems]=useState<Item[]>([]),[offset,setOffset]=useState(0),[total,setTotal]=useState(0),[error,setError]=useState(''),[loading,setLoading]=useState(false);
 useEffect(()=>{setOffset(0);},[value]);
 useEffect(()=>{
  if(!status.vault){setItems([]);setTotal(0);setOpen(false);setError('');return;}
  if(!open||status.busy)return;
  let active=true;setLoading(true);
  const timer=setTimeout(()=>{void listItems(value,offset,10).then(page=>{
   if(active){setItems(page.items);setTotal(page.total);setError('');}
  }).catch(()=>{if(active)setError('Could not load your archive. You can enter a snapshot name.');}).finally(()=>{if(active)setLoading(false);});},150);
  return()=>{active=false;clearTimeout(timer);};
 },[open,value,offset,status.vault?.path,status.vault?.itemCount,status.busy]);
 return <div className="field snapshot-picker">
  <label htmlFor={id}>Source snapshot</label>
  <div className="snapshot-input"><FolderGit2 size={18}/><input id={id} aria-label={label} autoComplete="off" required maxLength={120} value={value} placeholder="Search your captured repositories" onChange={e=>{onChange(e.target.value);setOpen(true);}} onKeyDown={e=>{if(e.key==='Escape'){setOpen(false);e.stopPropagation();}}}/><button type="button" aria-label="Browse saved snapshots" aria-expanded={open} aria-controls={`${id}-results`} disabled={!status.vault||status.busy} onClick={()=>setOpen(!open)}><ChevronDown size={18}/></button></div>
  <small>Choose from your encrypted archive, or enter a captured snapshot’s name.</small>
  {open&&status.vault&&<div id={`${id}-results`} className="snapshot-results" aria-label="Saved snapshot suggestions">
   <div className="snapshot-search-heading"><Search size={13}/><span>{loading?'Searching your archive…':`${total} archived items`}</span><button type="button" onClick={()=>setOpen(false)}>Close</button></div>
   {error&&<p role="alert">{error}</p>}
   {!loading&&!error&&items.length===0&&<p>No matching items. Capture a repository first, or try another name.</p>}
   {!loading&&items.map(item=><button type="button" className="snapshot-option" key={item.name} onClick={()=>{onChange(item.name);setOpen(false);}}><FolderGit2 size={17}/><span><strong>{item.name}</strong><small>{sizeLabel(item.size)} · encrypted archive item</small></span>{item.name===value&&<Check size={16}/>}</button>)}
   {total>10&&<div className="snapshot-pages"><button type="button" disabled={offset===0||loading} onClick={()=>setOffset(Math.max(0,offset-10))}>Previous snapshots</button><span>{offset+1}–{Math.min(offset+10,total)}</span><button type="button" disabled={offset+10>=total||loading} onClick={()=>setOffset(offset+10)}>More snapshots</button></div>}
   <p className="snapshot-hint">Migration receipts are available in History. The selected entry is checked before any repository operation.</p>
  </div>}
 </div>;
}
