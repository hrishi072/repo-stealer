import {useEffect,useRef,useState,type FormEvent} from 'react';
import {ArrowRight,Check,FileKey2,FolderOpen,HardDrive,LockKeyhole,ShieldCheck,X} from 'lucide-react';
import {choosePath,desktop,getWorkspaceSetup,type Action,type Status} from './api';

export function WorkspaceSetup({status,onClose,onSubmit,onAdvanced}:{status:Status;onClose:()=>void;onSubmit:(action:Action)=>Promise<void>;onAdvanced:()=>void}){
 const linux=status.platform==='linux',managed=!!status.workspace?.container;
 const [mode,setMode]=useState<'create'|'open'>('create'),[image,setImage]=useState(''),[capacity,setCapacity]=useState(4),[passphrase,setPassphrase]=useState(''),[confirm,setConfirm]=useState(''),[available,setAvailable]=useState(!linux),[checking,setChecking]=useState(true),[submitting,setSubmitting]=useState(false),[error,setError]=useState('');
 const panel=useRef<HTMLDivElement>(null),working=useRef(false);working.current=submitting;
 useEffect(()=>{
  let active=true;void getWorkspaceSetup().then(info=>{if(active){setAvailable(info.serviceAvailable);setImage(info.recommendedImage??'');if(info.containerExists)setMode('open');}}).catch(()=>{if(active)setError('Could not check the system disk service. You can still choose an existing encrypted folder.');}).finally(()=>{if(active)setChecking(false);});
  return()=>{active=false;};
 },[]);
 useEffect(()=>{
  const before=document.activeElement as HTMLElement|null;
  panel.current?.querySelector<HTMLElement>('button:not(:disabled)')?.focus();
  const keys=(e:KeyboardEvent)=>{
   if(e.key==='Escape'&&!working.current)onClose();
   if(e.key==='Tab'){
    const nodes=[...(panel.current?.querySelectorAll<HTMLElement>('button:not(:disabled),input:not(:disabled),select:not(:disabled),a[href]')??[])];
    if(e.shiftKey&&document.activeElement===nodes[0]){e.preventDefault();nodes.at(-1)?.focus();}
    else if(!e.shiftKey&&document.activeElement===nodes.at(-1)){e.preventDefault();nodes[0]?.focus();}
   }
  };
  window.addEventListener('keydown',keys);return()=>{window.removeEventListener('keydown',keys);if(before?.isConnected)before.focus();};
 },[onClose]);
 async function browse(){try{const path=await choosePath(mode==='create'?'workspace_save':'workspace_open');if(path)setImage(path);}catch{setError('Could not open the file chooser. You can enter the container path.');}}
 async function submit(e:FormEvent){e.preventDefault();setError('');if(linux&&mode==='create'&&passphrase!==confirm){setError('The storage passphrases do not match.');return;}setSubmitting(true);try{
  await onSubmit(linux?mode==='create'?{kind:'create_protected_workspace',imagePath:image,capacityGib:capacity,passphrase}:{kind:'open_protected_workspace',imagePath:image,passphrase}:{kind:'create_default_workspace'});
 }catch(e){setError(e instanceof Error?e.message:String(e));}finally{setPassphrase('');setConfirm('');setSubmitting(false);}}
 async function closeStorage(){setSubmitting(true);setError('');try{await onSubmit({kind:'close_protected_workspace'});}catch(e){setError(e instanceof Error?e.message:String(e));}finally{setSubmitting(false);}}
 return <div className="modal-backdrop" onMouseDown={e=>{if(e.target===e.currentTarget&&!submitting)onClose();}}><div ref={panel} className="modal storage-setup" role="dialog" aria-modal="true" aria-labelledby="storage-title">
 <div className="modal-heading"><span className="modal-symbol"><ShieldCheck size={24}/></span><button type="button" className="close-button" aria-label="Close storage setup" disabled={submitting} onClick={onClose}><X size={19}/></button><span className="mini-eyebrow">ONE-TIME SETUP. NO TERMINAL.</span><h2 id="storage-title">A protected place to work</h2><p>Your vault protects saved history. Working storage protects temporary Git files and restored repositories while you use them.</p></div>
 <form onSubmit={e=>void submit(e)}><div className="modal-body">
 {!desktop&&<div className="notice compact">Open the native desktop app to create or unlock protected storage.</div>}
 {managed?<div className="storage-ready"><ShieldCheck size={32}/><h3>Protected storage is ready</h3><p className="path">{status.workspace?.path}</p><p>You can capture, restore and migrate repositories now. Locking the vault alone does not close this container.</p><button type="button" className="button secondary" disabled={submitting||status.busy} onClick={()=>void closeStorage()}><LockKeyhole size={16}/>Lock protected storage</button><small>Close repository files and folders first. The app never forces an unmount.</small></div>:<>
 {linux?<><div className="storage-options"><button type="button" aria-pressed={mode==='create'} onClick={()=>{setMode('create');setPassphrase('');setConfirm('');setError('');}}><HardDrive size={21}/><strong>Create protected storage</strong><small>A new encrypted file on your computer</small></button><button type="button" aria-pressed={mode==='open'} onClick={()=>{setMode('open');setPassphrase('');setConfirm('');setError('');}}><FileKey2 size={21}/><strong>Unlock an existing container</strong><small>Use your storage passphrase after restart</small></button></div>
 <div className="storage-explainer"><Check size={16}/><span>{mode==='create'?'Creates LUKS2 encryption and ext4 inside a new file. Existing files and physical drives are never formatted.':'Opens an existing encrypted container. The app never formats or replaces it.'}</span></div>
 {checking?<p role="status">Checking your system disk service…</p>:!available&&<div className="form-error" role="alert">UDisks2 with ext4/encryption support is required. Install UDisks2, cryptsetup and ext4 tools through your system software manager, or use an existing encrypted folder below.</div>}
 <label className="field"><span>Protected storage file</span><div className="path-input"><input aria-label="Protected storage file" required value={image} onChange={e=>setImage(e.target.value)} autoComplete="off" placeholder="Choose a .rsworkspace file"/><button type="button" aria-label="Browse protected storage file" onClick={()=>void browse()}><FolderOpen size={17}/></button></div><small>{mode==='create'?'Choose a new filename. Keep this file to reopen your storage later.':'Choose the same .rsworkspace file you created earlier.'}</small></label>
 {mode==='create'&&<label className="field"><span>Maximum capacity</span><select aria-label="Protected storage capacity" value={capacity} onChange={e=>setCapacity(Number(e.target.value))}>{[1,2,4,8,16,32,64].map(n=><option key={n} value={n}>{n} GiB</option>)}</select><small>A sparse container grows as data is written, up to this limit. Keep enough free space on the drive.</small></label>}
 <label className="field"><span>Storage passphrase</span><input aria-label="Storage passphrase" type="password" required minLength={mode==='create'?12:undefined} maxLength={4096} autoComplete="off" value={passphrase} onChange={e=>setPassphrase(e.target.value)}/><small>This unlocks working storage, separately from your vault. It is never saved and cannot be reset.</small></label>
 {mode==='create'&&<label className="field"><span>Confirm storage passphrase</span><input aria-label="Confirm storage passphrase" type="password" required autoComplete="off" value={confirm} onChange={e=>setConfirm(e.target.value)}/></label>}
 <p className="storage-auth-hint">Your operating system may show its normal authorization dialog. Creating encryption can take a minute. Keep the app open until setup finishes.</p>
 </>:<div className="storage-ready"><ShieldCheck size={34}/><h3>Let Windows protect the folder</h3><p>We’ll create a private working folder in your local app data, enable Windows EFS, and verify that new files inherit encryption.</p><p>No container passphrase or terminal is needed. Windows EFS support is required; BitLocker alone does not replace this check.</p></div>}
 </>}
 {error&&<div className="form-error" role="alert">{error}</div>}
 <button type="button" className="text-button storage-advanced" disabled={submitting} onClick={onAdvanced}>Use an existing encrypted folder <ArrowRight size={14}/></button>
 </div><div className="modal-footer"><button type="button" className="button ghost" disabled={submitting} onClick={onClose}>{managed?'Done':'Cancel'}</button>{!managed&&<button className="button primary" disabled={!desktop||checking||!available||submitting||status.busy}><ShieldCheck size={16}/>{linux?mode==='create'?'Create and use protected storage':'Unlock and use protected storage':'Create encrypted working folder'}</button>}</div></form>
 </div></div>;
}
