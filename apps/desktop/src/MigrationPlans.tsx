import { useEffect, useState, type FormEvent } from 'react';
import {SnapshotPicker} from './SnapshotPicker';
import { FolderOpen, GitBranch, ShieldCheck } from 'lucide-react';
import { choosePath, listPlanRows, type Action, type PlanRow, type Status } from './api';

const words = (text: string) => text.replaceAll('_', ' ');
export function MigrationPlans({ status, onSubmit }: { status: Status; onSubmit: (action: Action) => Promise<void> }) {
  const [name, setName] = useState('');
  const [provider, setProvider] = useState<'github'|'gitlab'>('github');
  const [selfHosted, setSelfHosted] = useState(false);
  const [instance, setInstance] = useState('');
  const [repository, setRepository] = useState('');
  const [anonymous, setAnonymous] = useState(false);
  const [token, setToken] = useState('');
  const [username, setUsername] = useState('');
  const [caFile, setCaFile] = useState('');
  const [error, setError] = useState('');
  const [submitting, setSubmitting] = useState(false);
  const [rows, setRows] = useState<PlanRow[]>([]);
  const [total, setTotal] = useState(0);
  const [offset, setOffset] = useState(0);
  const [query, setQuery] = useState('');
  const plan = status.plan;
  const ready = !!status.vault && !!status.workspace && !status.busy && !submitting;
  const identity = plan ? `${plan.observedAt}:${plan.sourceSha256}:${plan.destinationFingerprint}:${plan.requestedRepository}` : '';
  useEffect(() => { setOffset(0); setQuery(''); }, [identity]);
  useEffect(() => {
    setRows([]); setTotal(0);
    if (!status.vault || !plan) return;
    let active = true;
    void listPlanRows(query, offset).then(page => { if (active) { setRows(page.rows); setTotal(page.total); } }).catch(() => { if (active) setError('Cannot load plan rows. Prepare the plan again.'); });
    return () => { active = false; };
  }, [identity, query, offset, !!status.vault]); // summary polling never reloads full ref lists
  useEffect(() => {
    if (!status.vault) { setToken(''); setName(''); setRepository(''); setInstance(''); setUsername(''); setCaFile(''); setError(''); }
  }, [!!status.vault]);
  async function submit(event: FormEvent) {
    event.preventDefault(); setError(''); setSubmitting(true);
    try {
      await onSubmit({ kind: 'prepare_plan', name, provider, instance: selfHosted ? instance : null, repository, token: anonymous ? null : token, username: username || null, caFile: caFile || null });
      setToken('');
    } catch (error) { setError(error instanceof Error ? error.message : String(error)); }
    finally { setSubmitting(false); }
  }
  return <div className="migration-plans">
    <div className="notice"><ShieldCheck size={20}/><div><strong>Read-only destination preflight</strong><p>No repository creation, pushes, deletions or default-branch changes. Plans stay in this session and clear when you lock the vault. Roles and policy listings cannot prove write authorization.</p></div></div>
    <section className="panel"><div className="panel-heading"><div><h2>Prepare a migration plan</h2><p>Unlock a vault and verify encrypted working storage first. Enter an existing destination’s canonical repository path.</p></div></div>
      <form onSubmit={event => void submit(event)} className="plan-form">
        <SnapshotPicker status={status} value={name} onChange={setName} label="Plan source snapshot"/>
        <label className="field"><span>Provider</span><select aria-label="Destination provider" value={provider} onChange={event => setProvider(event.target.value as 'github'|'gitlab')}><option value="github">GitHub</option><option value="gitlab">GitLab</option></select></label>
        <label className="checkbox-field"><input type="checkbox" checked={selfHosted} onChange={event => setSelfHosted(event.target.checked)}/> Self-hosted instance</label>
        {selfHosted && <label className="field"><span>Instance HTTPS URL</span><input aria-label="Destination instance" type="url" required value={instance} onChange={event => setInstance(event.target.value)} placeholder="https://git.example.com/platform"/><small>Web instance root, including any relative URL prefix. API paths are added automatically.</small></label>}
        <label className="field"><span>Destination repository path</span><input aria-label="Destination repository" required maxLength={1024} value={repository} onChange={event => setRepository(event.target.value)} placeholder={provider === 'github' ? 'owner/repository' : 'group/subgroup/project'} autoComplete="off"/></label>
        <label className="checkbox-field"><input type="checkbox" checked={anonymous} onChange={event => { setAnonymous(event.target.checked); setToken(''); setUsername(''); }}/> Anonymous read-only access</label>
        {!anonymous && <><label className="field"><span>Destination access token</span><input aria-label="Destination access token" type="password" required maxLength={4096} value={token} onChange={event => setToken(event.target.value)} autoComplete="off"/><small>The same token reads provider metadata and Git refs. It is cleared after starting the job.</small></label><label className="field"><span>Git username (optional)</span><input aria-label="Destination Git username" value={username} onChange={event => setUsername(event.target.value)} autoComplete="off"/><small>Uses the API actor login when available; otherwise a provider token username.</small></label></>}
        <label className="field"><span>Private CA bundle (optional)</span><div className="path-input"><input aria-label="Destination CA bundle" value={caFile} onChange={event => setCaFile(event.target.value)}/><button type="button" aria-label="Browse destination CA bundle" onClick={() => void choosePath('key_open').then(path => { if (path) setCaFile(path); }).catch(() => setError('Cannot open the certificate picker.'))}><FolderOpen size={17}/></button></div><small>TLS stays verified. Redirects are refused.</small></label>
        {error && <p role="alert" className="job-error">{error}</p>}
        <button className="button primary" disabled={!ready}><GitBranch size={17}/> Prepare read-only plan</button>
      </form>
    </section>
    {plan && status.vault && <section className="panel" aria-label="Migration plan result"><div className="panel-heading"><div><h2>Destination preview</h2><p>{plan.provider} · {plan.deployment === 'cloud' ? 'Cloud' : 'Self-hosted'} · {plan.requestedRepository}</p></div><span className="badge warning">Write authorization unproven</span></div>
      <div className="plan-details"><p>Source: <strong>{plan.sourceName}</strong> · {words(plan.sourceVerification)} · {new Date(plan.observedAt * 1000).toLocaleString()}</p><p>Actor: {plan.actor ? `${plan.actor.login} (ID ${plan.actor.id})` : 'Unavailable / anonymous'}</p><p>Repository ID: {plan.repository?.id ?? 'Unavailable'} · Default branch: {plan.repository?.defaultBranch ?? 'Unknown'} · Push role: {plan.repository?.pushReported === true ? 'Reported by API; token scope unproven' : plan.repository?.pushReported === false ? 'Denied by repository role' : 'Unknown'} · GitLab access level: {plan.repository?.accessLevel ?? 'Unknown'}</p>
        <p>{plan.sourceExcludedRefs} source and {plan.destinationExcludedRefs} destination nonportable refs excluded. Heads and tags only.</p>
        <p className="path">Destination observation fingerprint: {plan.destinationFingerprint ?? 'Unavailable'}</p>
        {plan.blockers.length > 0 && <div role="alert"><h3>Blockers</h3><ul>{plan.blockers.map(item => <li key={item}>{words(item)}</li>)}</ul></div>}
        <details open><summary>Limits and review required</summary><ul>{plan.warnings.map(item => <li key={item}>{words(item)}</li>)}</ul></details>
        <ul>{plan.policies.map(policy => <li key={policy.endpoint}>{words(policy.endpoint)}: {policy.count} listed · {policy.complete ? 'Listing complete; manual policy evaluation required' : 'Coverage incomplete / unknown'}{policy.issue ? ` · ${words(policy.issue.code)}` : ''}</li>)}</ul>
        {plan.apiIssues.map((issue, index) => <p key={index}>{words(issue.code)}{issue.status ? ` (HTTP ${issue.status})` : ''}{issue.retryAfterSeconds !== null ? ` · retry after ${issue.retryAfterSeconds}s` : ''}</p>)}
      </div>
      <div className="table-tools"><label className="search"><input aria-label="Search plan refs" value={query} onChange={event => { setQuery(event.target.value); setOffset(0); }} placeholder="Search plan refs…"/></label><span>{total} refs</span></div>
      <div className="table-scroll"><table><thead><tr><th>REF</th><th>PREVIEW</th><th>SOURCE</th><th>DESTINATION</th></tr></thead><tbody>{rows.map(row => <tr key={row.name}><td className="path">{row.name}</td><td>{words(row.change)}</td><td className="mono" title={row.sourceOid ?? ''}>{row.sourceOid?.slice(0, 12) ?? '—'}</td><td className="mono" title={row.destinationOid ?? ''}>{row.destinationOid?.slice(0, 12) ?? '—'}</td></tr>)}</tbody></table></div>
      {!plan.rows && <p className="plan-details">No ref comparison is available. An unavailable destination is never assumed empty.</p>}
      {total > 20 && <div className="pagination"><span>{offset + 1}–{Math.min(offset + 20, total)} of {total}</span><div><button disabled={!offset} onClick={() => setOffset(Math.max(0, offset - 20))}>Previous</button><button disabled={offset + 20 >= total} onClick={() => setOffset(offset + 20)}>Next</button></div></div>}
    </section>}
  </div>;
}
