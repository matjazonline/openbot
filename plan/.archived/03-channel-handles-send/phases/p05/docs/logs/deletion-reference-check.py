from pathlib import Path
assert not Path('plan/local_machine_tunnel_plan.md').exists()
assert 'local_machine_tunnel_plan.md' not in Path('src/main.rs').read_text()
assert 'local_machine_tunnel_plan.md' not in Path('plan/cloudroom-local/README.md').read_text()
print('Deleted plan absent; startup warning and live documentation have no stale reference.')
