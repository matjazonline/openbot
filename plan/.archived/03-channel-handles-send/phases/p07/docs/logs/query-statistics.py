import os,subprocess,re
url=os.environ["DATABASE_URL"]
def query(sql):
    return subprocess.check_output(["psql",url,"-X","-v","ON_ERROR_STOP=1","-Atqc",sql],text=True).strip()
for name,expected in [("pg_stat_statements.track","top"),("pg_stat_statements.track_utility","off"),("log_parameter_max_length","0"),("log_parameter_max_length_on_error","0")]:
    actual=query("SHOW "+name)
    assert actual==expected,(name,actual,expected)
    print(name,actual)
query("SELECT 1 AS ci_pg_stat_statements_probe")
identity=query("SELECT queryid FROM pg_stat_statements WHERE dbid=(SELECT oid FROM pg_database WHERE datname=current_database()) AND query LIKE '%ci_pg_stat_statements_probe%' LIMIT 1")
assert re.fullmatch(r"-?[0-9]+",identity),identity
print("CI query-statistics probe observed; numeric queryid",identity)
