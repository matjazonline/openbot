"""Parse the pg_dump part of the init migration into header segments."""
import re
M = '/Users/mac02/dev/openbot/migrations/20260817000000_init_schema.sql'

def load():
    lines = open(M).read().split('\n')
    tail = next(i for i, l in enumerate(lines) if l == '-- PostgreSQL database dump complete') - 1
    starts = [i for i in range(tail) if lines[i] == '--' and lines[i+1].startswith('-- Name: ') and lines[i+2] == '--']
    segs = []
    for n, s in enumerate(starts):
        e = starts[n+1] if n+1 < len(starts) else tail
        m = re.match(r'-- Name: (.*); Type: ([A-Z ]+); Schema: ([^;]*); Owner: (.*)', lines[s+1])
        segs.append(dict(start=s, end=e, name=m.group(1), type=m.group(2), body='\n'.join(lines[s+3:e])))
    return lines, segs, tail
