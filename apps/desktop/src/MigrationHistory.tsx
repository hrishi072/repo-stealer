import {useEffect,useState,type FormEvent} from 'react';
import {listMigrationHistory,type Action,type HistoryItem,type Status} from './api';

export function MigrationHistoryView({status,onSubmit}:{status:Status;onSubmit:(action:Action)=>Promise<void>}) {
 const [items,setItems]=useState<HistoryItem[]>([]),[offset,setOffset]=useState(0),[total,setTotal]=useState(0);
 const [token,setToken]=useState(''),[username,setUsername]=useState(''),[ca,setCa]=useState(''),[error,setError]=useState(''),[loading,setLoading]=useState(false);
 const record=status.migrationHistory, unlocked=!!status.vault;
 const ready=unlocked && !status.busy && !loading;
 useEffect(()=>{
  setToken('');setUsername('');setCa('');setError('');
  if(!unlocked){setItems([]);setTotal(0);setOffset(0);}
 },[unlocked,record?.plan.id]);
 useEffect(()=>{
  if(!unlocked || status.busy)return;
  let active=true;
  void listMigrationHistory(offset).then(page=>{if(active){setItems(page.items);setTotal(page.total);setError('');}}).catch(()=>{if(active)setError('Could not read migration history.');});
  return ()=>{active=false;};
 },[unlocked,status.busy,status.vault?.path,status.vault?.itemCount,offset]);
 async function inspect(planId:string){setLoading(true);setError('');try{await onSubmit({kind:'inspect_migration',planId});}catch{setError('Could not open migration record.');}finally{setLoading(false);}}
 async function reconcile(event:FormEvent){event.preventDefault();if(!record)return;setLoading(true);setError('');try{await onSubmit({kind:'reconcile_migration',planId:record.plan.id,token,username:username||null,caFile:ca||null});}catch{setError('Could not start destination observation.');}finally{setToken('');setLoading(false);}}
 if(!unlocked)return <section className="panel"><div className="empty-state"><h2>Unlock your vault to read migration history</h2><p>Plans, receipts and saved observations stay encrypted between sessions.</p></div></section>;
 return <div className="migration-plans">
 <div className="notice"><div><strong>Read-only recovery reports</strong><p>Compare recorded repository identity, private state, disabled automation and advertised refs. Matching refs do not reverify Git objects or LFS payloads. Reports never authorize retry, adoption, rollback or resume.</p></div></div>
 {error && <p role="alert" className="job-error">{error}</p>}
 <section className="panel" aria-label="Saved migrations"><div className="panel-heading"><h2>Saved migrations</h2><span>{total} plans</span></div>
 <div className="table-scroll"><table><thead><tr><th>Destination / plan</th><th>Recorded state</th><th>Receipts</th></tr></thead><tbody>{items.map(item=><tr key={item.id}><td><span>{item.repository??'Unreadable record'}</span><div className="path">{item.id}</div></td><td>{item.state.replaceAll('_',' ')}</td><td><button className="button secondary small-button" disabled={!ready || item.state==='unreadable'} onClick={()=>void inspect(item.id)} aria-label={`Inspect migration ${item.id}`}>Inspect receipts</button></td></tr>)}</tbody></table></div>
 {!items.length && <p className="plan-details">No saved migration plans on this page.</p>}
 <div className="panel-heading"><button className="button secondary small-button" disabled={!ready||offset===0} onClick={()=>setOffset(Math.max(0,offset-20))}>Previous migrations</button><span>{total?`${offset+1}–${Math.min(offset+20,total)} of ${total}`:'0 plans'}</span><button className="button secondary small-button" disabled={!ready||offset+20>=total} onClick={()=>setOffset(offset+20)}>Next migrations</button></div></section>
 {record && <section className="panel" aria-label="Saved migration details"><div className="panel-heading"><h2>{record.plan.repository}</h2><span>{record.state.replaceAll('_',' ')}</span></div><div className="plan-details">
 <p className="path">{record.plan.instance} · {record.plan.provider} · Plan {record.plan.id}</p>
 <p>Recorded repository ID: {record.repositoryId??'not confirmed'} · Last durable phase: {record.lastPhase}</p>
 <p className="path">Source: {record.plan.sourceName} · SHA-256: {record.plan.sourceSha256}</p>
 <p>{record.plan.refCount} heads/tags · default {record.plan.defaultBranch} · {record.plan.lfsObjects??0} archived LFS objects</p>
 <ol>{record.receipts.map(receipt=><li key={receipt.phase}>{receipt.phase} · {new Date(receipt.time*1000).toLocaleString()}{receipt.failureCode && ` · ${receipt.failureCode}`}</li>)}</ol>
 {!record.receipts.length && <p>No execution intent was recorded.</p>}
 <details><summary>Recorded source refs</summary><pre className="path">{Object.entries(record.plan.refs).map(([name,oid])=>`${name} → ${oid}`).join('\n')}</pre></details>
 </div><form className="plan-form" onSubmit={e=>void reconcile(e)}>
 <label className="field"><span>Destination read token</span><input aria-label="Reconciliation token" type="password" maxLength={4096} autoComplete="off" required value={token} onChange={e=>setToken(e.target.value)}/><small>Used only for this observation. Never saved.</small></label>
 <label className="field"><span>Git username (optional)</span><input aria-label="Reconciliation username" value={username} onChange={e=>setUsername(e.target.value)}/></label>
 <label className="field"><span>Private CA PEM bundle (optional)</span><input aria-label="Reconciliation CA" value={ca} onChange={e=>setCa(e.target.value)}/></label>
 <button className="button primary" disabled={!ready||!status.workspace}>Observe destination and save encrypted report</button><small>Verified encrypted working storage is required. Missing durable repository identity is reported without adopting the destination.</small>
 </form>
 <div className="plan-details" aria-label="Saved reconciliation reports"><h3>Saved observations</h3><p>Up to 10 recent reports. The original execution result remains unchanged.</p>{record.reconciliations.map((report,i)=><section key={report.reportId??`${report.observedAt}-${i}`}><h4>{report.state.replaceAll('_',' ')} · {new Date(report.observedAt*1000).toLocaleString()}</h4>{report.issues.map(issue=><p key={issue}>{issue.replaceAll('_',' ')}</p>)}{report.differences.length>0 && <details><summary>{report.differenceCount??report.differences.length} ref discrepancies (first 200 shown)</summary><pre className="path">{report.differences.map(row=>`${row.name}\nRecorded: ${row.expectedOid??'absent'}\nObserved: ${row.observedOid??'absent'}`).join('\n\n')}</pre></details>}</section>)}</div>
 </section>}
 </div>;
}
