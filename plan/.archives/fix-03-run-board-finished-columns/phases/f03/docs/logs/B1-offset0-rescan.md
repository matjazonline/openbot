# Board counts behind an `OFFSET 0` fence: rescanned per version (A3 revision)

Found while answering F03-B1-F1. The script gained a "recent" agent (`md5('agent1-40')`, in the
last versions of workflows 1 and 32), and its counts under index (B), `plan_cache_mode = auto`,
took 194 ms and 111,055 buffers. The planner estimated one matching version (there are 13), put
the versions on the outer side of a nested loop, and re-executed the fenced working-set subquery
for each of them: 13 × 12,500 open runs. An `OFFSET 0` subquery keeps the scope out of the working
set's scan, but a rescan of it runs it again. The statement now uses `WITH working AS
MATERIALIZED (...)`, which runs at most once (a rescan reads its stored rows); the same read
then takes 17.7 ms and 8,575 buffers in both plan modes (`docs/performance/run-board-query-plan.txt`).

Excerpt of the plan from that run (full output was overwritten by the re-run):

```
== board_counts_agent recent heavy week [(B) (company_id, state, finished_at DESC, id DESC) top-level] (plan_cache_mode = auto)
 GroupAggregate  (cost=33549.62..33549.64 rows=1 width=32) (actual time=194.141..194.181 rows=9.00 loops=1)
   Group Key: workflow_runs.state, workflow_runs.channel_id
   Buffers: local hit=965 read=110090
   ->  Sort  (actual time=194.137..194.145 rows=417.00 loops=1)
         ->  Nested Loop  (cost=187.91..33549.61 rows=1 width=24) (actual time=112.239..194.033 rows=417.00 loops=1)
               Join Filter: (workflow_runs.version_id = version.id)
               Rows Removed by Join Filter: 185119
               ->  Bitmap Heap Scan on workflow_versions version  (cost=34.62..1796.39 rows=1 width=16) (actual time=0.036..0.165 rows=13.00 loops=1)
                     Filter: ('b9ab4784-a186-9716-e09e-fe1e9ea88f9d'::uuid = ANY (agent_ids))
                     Rows Removed by Filter: 787
               ->  Append  (cost=153.29..31468.05 rows=12674 width=56) (actual time=1.272..14.463 rows=14272.00 loops=13)
                     Buffers: local hit=962 read=110058
                     ->  Bitmap Heap Scan on workflow_runs  (actual time=1.271..13.664 rows=12500.00 loops=13)
                           Recheck Cond: ((company_id = ...) AND (state = ANY ('{queued,running,waiting}'::text[])))
                           Filter: (parent_run_id IS NULL)
                           Heap Blocks: exact=109512
                     ->  Bitmap Heap Scan on workflow_runs workflow_runs_1  (actual time=0.107..0.269 rows=1772.00 loops=13)
                           ->  Bitmap Index Scan on board_finished_b
 Execution Time: 194.315 ms
```
