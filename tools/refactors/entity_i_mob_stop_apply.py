#!/usr/bin/env python3
"""Extract the actual post-veto mob detach kernel for removal reuse."""
from pathlib import Path

path = Path(__file__).resolve().parents[2] / 'crates/horizon-client/src/game/modding/native_topology_plans.rs'
s = path.read_text()
if 'fn apply_topology_mob_stop(' not in s:
    start = s.index('    pub(super) fn execute_topology_mob_stop(')
    brace = s.index('{', start)
    depth, end = 1, brace + 1
    while depth:
        if s[end] == '{': depth += 1
        elif s[end] == '}': depth -= 1
        end += 1
    tail_start = s.index('        self.with_topology_mob_mut(fresh.rider.admission, |mob| mob.extra.mount = None)?;', brace, end)
    tail = s[tail_start:end-1]
    before = s[start:tail_start].replace(
        '        use super::{\n            native_base_read_rows::BaseReadSource, native_base_read_source::base_read_source_kind,\n        };',
        '        use super::native_base_read_source::base_read_source_kind;')
    helper = ('    fn apply_topology_mob_stop(&mut self, fresh: StopPlan) -> Result<bool, EntityReadError> {\n'
              '        use super::native_base_read_rows::BaseReadSource;\n'
              '        let entity = fresh.rider.admission.entity();\n' + tail + '}')
    s = s[:start] + before + '        self.apply_topology_mob_stop(fresh)\n    }\n\n' + helper + s[end:]
    path.write_text(s)
