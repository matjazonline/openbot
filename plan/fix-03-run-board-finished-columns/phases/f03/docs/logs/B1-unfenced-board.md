# Board statements before the OFFSET 0 fence (decision A3)

Same script and dataset as docs/performance/run-board-query-plan.txt, run 2026-10-31 with the scope
filters (binding, agent) inside each statement instead of outside a fenced working set. Columns:
time / buffers (max over plan nodes) / rows removed by filter. Full output was not kept (1.3 MB);
the two plans below show the shape.

| Read | none | (A) | (B) generic | (B) auto |
| --- | ---: | ---: | ---: | ---: |
| board_counts_binding heavy week | 24.1 ms / 12,536 buf / 6,250 filtered | 23.4 ms / 12,536 buf / 6,250 filtered | 23.9 ms / 12,536 buf / 6,250 filtered | 23.7 ms / 12,536 buf / 6,250 filtered |
| board_cards_binding heavy week all visible | 35.3 ms / 19,272 buf / 19,053 filtered | 33.2 ms / 19,272 buf / 19,053 filtered | 34.2 ms / 19,272 buf / 19,053 filtered | 36.9 ms / 19,272 buf / 19,053 filtered |
| board_cards_binding heavy week 8 of 32 visible, no company runs | 35.4 ms / 19,272 buf / 19,053 filtered | 33.2 ms / 19,272 buf / 19,053 filtered | 34.5 ms / 19,272 buf / 19,053 filtered | 34.7 ms / 19,272 buf / 19,053 filtered |
| board_counts_agent dense heavy week | 64.1 ms / 230,378 buf / 1,347 filtered | 66.4 ms / 230,378 buf / 1,347 filtered | 63.6 ms / 230,378 buf / 1,347 filtered | 64.2 ms / 230,378 buf / 1,347 filtered |
| board_counts_agent sparse heavy week | 1.0 ms / 702 buf / 1,911 filtered | 1.1 ms / 702 buf / 1,911 filtered | 1.0 ms / 702 buf / 1,911 filtered | 1.0 ms / 702 buf / 1,911 filtered |
| board_cards_agent dense heavy week all visible | 131.0 ms / 408,108 buf / 2,044 filtered | 131.4 ms / 408,108 buf / 2,044 filtered | 123.9 ms / 408,108 buf / 2,044 filtered | 172.4 ms / 691,134 buf / 4,071 filtered |
| board_cards_agent dense heavy week 8 of 32 visible, no company runs | 113.4 ms / 376,842 buf / 2,043 filtered | 110.3 ms / 376,842 buf / 2,043 filtered | 107.6 ms / 376,842 buf / 2,043 filtered | 17.0 ms / 32,765 buf / 1,463 filtered |
| board_cards_agent sparse heavy week all visible | 38.9 ms / 63,594 buf / 3,339 filtered | 38.3 ms / 63,594 buf / 3,339 filtered | 37.5 ms / 63,594 buf / 3,339 filtered | 1.5 ms / 2,106 buf / 6,359 filtered |
| board_cards_agent sparse heavy week 8 of 32 visible, no company runs | 18.5 ms / 32,328 buf / 3,339 filtered | 18.7 ms / 32,328 buf / 3,339 filtered | 18.4 ms / 32,328 buf / 3,339 filtered | 16.2 ms / 32,322 buf / 3,339 filtered |

## board_counts_agent dense heavy week, (B), generic

```
== board_counts_agent dense heavy week [(B) (company_id, state, finished_at DESC, id DESC) top-level] (plan_cache_mode = force_generic_plan)
                                                                                                     QUERY PLAN
---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------
 Append  (cost=2235.76..4471.49 rows=3 width=32) (actual time=33.841..63.369 rows=6.00 loops=1)
   Buffers: local hit=218002 read=12376
   ->  GroupAggregate  (cost=2235.76..2235.78 rows=1 width=32) (actual time=33.840..34.270 rows=6.00 loops=1)
         Group Key: workflow_runs.state, workflow_runs.channel_id
         Buffers: local hit=109156 read=6033
         ->  Sort  (cost=2235.76..2235.76 rows=1 width=24) (actual time=33.750..33.876 rows=6250.00 loops=1)
               Sort Key: workflow_runs.state, workflow_runs.channel_id
               Sort Method: quicksort  Memory: 437kB
               Buffers: local hit=109156 read=6033
               ->  Nested Loop  (cost=34.85..2235.75 rows=1 width=24) (actual time=0.058..32.678 rows=6250.00 loops=1)
                     Buffers: local hit=109156 read=6033
                     ->  Bitmap Heap Scan on workflow_versions version  (cost=34.43..1762.17 rows=39 width=16) (actual time=0.045..0.195 rows=400.00 loops=1)
                           Recheck Cond: (company_id = $2)
                           Filter: ($3 = ANY (agent_ids))
                           Rows Removed by Filter: 400
                           Heap Blocks: exact=27
                           Buffers: local read=36
                           ->  Bitmap Index Scan on workflow_versions_pkey  (cost=0.00..34.42 rows=800 width=0) (actual time=0.033..0.033 rows=800.00 loops=1)
                                 Index Cond: (company_id = $2)
                                 Index Searches: 1
                                 Buffers: local read=9
                     ->  Index Scan using workflow_runs_company_id_version_id_created_at_id_idx1 on workflow_runs  (cost=0.42..12.13 rows=1 width=40) (actual time=0.066..0.080 rows=15.62 loops=400)
                           Index Cond: ((company_id = $1) AND (version_id = version.id))
                           Filter: (state = ANY ('{queued,running,waiting}'::text[]))
                           Rows Removed by Filter: 266
                           Index Searches: 400
                           Buffers: local hit=109156 read=5997
   ->  GroupAggregate  (cost=2235.66..2235.70 rows=2 width=32) (actual time=29.097..29.098 rows=0.00 loops=1)
         Group Key: workflow_runs_1.state, workflow_runs_1.channel_id
         Buffers: local hit=108846 read=6343
         ->  Sort  (cost=2235.66..2235.66 rows=2 width=24) (actual time=29.097..29.097 rows=0.00 loops=1)
               Sort Key: workflow_runs_1.state, workflow_runs_1.channel_id
               Sort Method: quicksort  Memory: 25kB
               Buffers: local hit=108846 read=6343
               ->  Nested Loop  (cost=34.85..2235.65 rows=2 width=24) (actual time=29.094..29.095 rows=0.00 loops=1)
                     Buffers: local hit=108846 read=6343
                     ->  Bitmap Heap Scan on workflow_versions version_1  (cost=34.43..1762.17 rows=39 width=16) (actual time=0.043..0.187 rows=400.00 loops=1)
                           Recheck Cond: (company_id = $5)
                           Filter: ($6 = ANY (agent_ids))
                           Rows Removed by Filter: 400
                           Heap Blocks: exact=27
                           Buffers: local read=36
                           ->  Bitmap Index Scan on workflow_versions_pkey  (cost=0.00..34.42 rows=800 width=0) (actual time=0.034..0.034 rows=800.00 loops=1)
                                 Index Cond: (company_id = $5)
                                 Index Searches: 1
                                 Buffers: local read=9
                     ->  Index Scan using workflow_runs_company_id_version_id_created_at_id_idx1 on workflow_runs workflow_runs_1  (cost=0.42..12.13 rows=1 width=40) (actual time=0.072..0.072 rows=0.00 loops=400)
                           Index Cond: ((company_id = $4) AND (version_id = version_1.id))
                           Filter: (finished_at >= $7)
                           Rows Removed by Filter: 281
                           Index Searches: 400
                           Buffers: local hit=108846 read=6307
 Planning:
   Buffers: local hit=2 read=2
 Planning Time: 0.315 ms
 Execution Time: 63.594 ms
```

## board_counts_binding heavy week, (B), generic

```
== board_counts_binding heavy week [(B) (company_id, state, finished_at DESC, id DESC) top-level] (plan_cache_mode = force_generic_plan)
                                                                                                  QUERY PLAN
--------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------
 Append  (cost=23.32..46.70 rows=2 width=32) (actual time=12.077..23.894 rows=3.00 loops=1)
   Buffers: local hit=521 read=12015
   ->  GroupAggregate  (cost=23.32..23.35 rows=1 width=32) (actual time=12.077..12.479 rows=3.00 loops=1)
         Group Key: workflow_runs.state, workflow_runs.channel_id
         Buffers: local hit=428 read=5840
         ->  Sort  (cost=23.32..23.33 rows=1 width=24) (actual time=11.877..12.007 rows=6250.00 loops=1)
               Sort Key: workflow_runs.state, workflow_runs.channel_id
               Sort Method: quicksort  Memory: 437kB
               Buffers: local hit=428 read=5840
               ->  Index Scan using workflow_runs_company_id_binding_id_created_at_id_idx on workflow_runs  (cost=0.55..23.31 rows=1 width=24) (actual time=0.013..10.986 rows=6250.00 loops=1)
                     Index Cond: ((company_id = $1) AND (binding_id = $2))
                     Filter: ((parent_run_id IS NULL) AND (state = ANY ('{queued,running,waiting}'::text[])))
                     Index Searches: 1
                     Buffers: local hit=428 read=5840
   ->  GroupAggregate  (cost=23.32..23.34 rows=1 width=32) (actual time=11.414..11.414 rows=0.00 loops=1)
         Group Key: workflow_runs_1.state, workflow_runs_1.channel_id
         Buffers: local hit=93 read=6175
         ->  Sort  (cost=23.32..23.32 rows=1 width=24) (actual time=11.414..11.414 rows=0.00 loops=1)
               Sort Key: workflow_runs_1.state, workflow_runs_1.channel_id
               Sort Method: quicksort  Memory: 25kB
               Buffers: local hit=93 read=6175
               ->  Index Scan using workflow_runs_company_id_binding_id_created_at_id_idx on workflow_runs workflow_runs_1  (cost=0.55..23.31 rows=1 width=24) (actual time=11.412..11.412 rows=0.00 loops=1)
                     Index Cond: ((company_id = $3) AND (binding_id = $4))
                     Filter: ((parent_run_id IS NULL) AND (finished_at >= $5))
                     Rows Removed by Filter: 6250
                     Index Searches: 1
                     Buffers: local hit=93 read=6175
 Planning:
   Buffers: local hit=2 read=2
 Planning Time: 0.188 ms
 Execution Time: 23.904 ms
```
