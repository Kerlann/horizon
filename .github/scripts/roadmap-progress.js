// Keeps the progress block in the pinned roadmap issue (#771) current: per Phase, how many of the
// board's issues are Done, drawn as a bar. Counts issues only (a PR and the issue it closes are one
// piece of work). Rewrites only the text between the progress markers; the rest of the body is
// edited by hand.
// `github` carries PROJECT_TOKEN (board reads); `issues` carries the workflow's own token, which
// may edit issues (PROJECT_TOKEN may not).
module.exports = async ({ github, issues, core }) => {
  const OWNER = 'arismoko', REPO = 'horizon', NUMBER = 1, ISSUE = 771;
  const START = '<!-- progress:start -->', END = '<!-- progress:end -->';

  const proj = (await github.graphql(`query($o:String!,$n:Int!){user(login:$o){projectV2(number:$n){id
    field(name:"Phase"){... on ProjectV2SingleSelectField{options{name}}}}}}`, { o: OWNER, n: NUMBER })).user.projectV2;
  const phases = new Map(proj.field.options.map(o => [o.name, { done: 0, total: 0 }]));
  const none = { done: 0, total: 0 };
  for (let after = null; ;) {
    const page = (await github.graphql(`query($id:ID!,$a:String){node(id:$id){... on ProjectV2{items(first:100,after:$a){
      pageInfo{hasNextPage endCursor}
      nodes{content{__typename}
        s:fieldValueByName(name:"Status"){... on ProjectV2ItemFieldSingleSelectValue{name}}
        p:fieldValueByName(name:"Phase"){... on ProjectV2ItemFieldSingleSelectValue{name}}}}}}}`, { id: proj.id, a: after })).node.items;
    for (const n of page.nodes) {
      if (n.content?.__typename !== 'Issue') continue;
      const row = phases.get(n.p?.name) || none;
      row.total++;
      if (n.s?.name === 'Done') row.done++;
    }
    if (!page.pageInfo.hasNextPage) break;
    after = page.pageInfo.endCursor;
  }

  const bar = (done, total) => {
    const share = total ? done / total : 0;
    const cells = Math.round(share * 20);
    return `\`${'█'.repeat(cells)}${'░'.repeat(20 - cells)}\` ${Math.round(share * 100)}%`;
  };
  const rows = [...phases].filter(([, r]) => r.total);
  if (none.total) rows.push(['No phase yet', none]);
  const done = rows.reduce((s, [, r]) => s + r.done, 0), total = rows.reduce((s, [, r]) => s + r.total, 0);
  const stamp = new Date().toISOString().slice(0, 16).replace('T', ' ');
  const block = [START, '### Progress', '',
    `**Overall** ${bar(done, total)} · ${done} of ${total} issues done`, '',
    '| Phase | Progress | Done |', '|---|---|---|',
    ...rows.map(([name, r]) => `| ${name} | ${bar(r.done, r.total)} | ${r.done} / ${r.total} |`), '',
    `<sub>Counted from the [board](https://github.com/users/${OWNER}/projects/${NUMBER}) automatically every hour (last ${stamp} UTC). New issues add to the total, so a bar can move back as work is found.</sub>`,
    END].join('\n');

  const issue = (await issues.rest.issues.get({ owner: OWNER, repo: REPO, issue_number: ISSUE })).data;
  const body = issue.body || '';
  const from = body.indexOf(START), to = body.indexOf(END);
  if (from < 0 || to < from) { core.setFailed(`#${ISSUE} has no progress markers`); return; }
  const next = body.slice(0, from) + block + body.slice(to + END.length);
  if (next.replace(/\(last [^)]*\)/, '') === body.replace(/\(last [^)]*\)/, '')) { core.summary.addRaw('No change.').write(); return; }
  await issues.rest.issues.update({ owner: OWNER, repo: REPO, issue_number: ISSUE, body: next });
  core.summary.addRaw(`#${ISSUE} progress: ${done} of ${total} issues done.`).write();
};
