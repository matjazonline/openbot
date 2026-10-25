#!/usr/bin/env python3
"""P09-D1: sample code may depend only on inert types and named pure engine functions.

This source gate supplements zero-effect behavioral tests. It expands Rust use groups and
checks qualified paths throughout every production sample module, including future files.
Test-only files follow the repository naming convention and cannot be mounted by production.
New dependencies require an explicit allowlist change and review; effect adapters are not allowed.
"""
from pathlib import Path
import re
import sys

PURE = {
    'WorkflowActor', 'activation::ActivatedExecution', 'activation::prepare_activation',
    'batch::PureCompletion', 'batch::execute_pure', 'batch::validate_completion',
    'completion::prepare_io', 'compiler::Diagnostic', 'compiler::Schema', 'compiler::SourceSpan',
    'children::RepeatRule', 'children::RoundDecision', 'children::child_completion',
    'children::decide_round', 'children::provisional_output',
    'decisions::DecisionCommandKey', 'decisions::DecisionId', 'decisions::SubmitDecision',
    'decisions::hex_digest', 'decisions::prepare_submission', 'decisions::reviewer_deadline',
    'publication::PublishedBundle', 'publication::ContentHash', 'publication::AgentKey',
    'publication::ToolKind', 'publication::ToolSnapshot', 'publication::ActionEffect',
    'publication::ToolKind::Workflow', 'publication::ActionEffect::Read', 'publication::ActionEffect::Write',
    'registry::ModelStep', 'registry::mcp_tool_result', 'mcp::mcp_result_is_error',
}
EXTERNAL = {'serde', 'serde_json', 'uuid', 'chrono'}
SAMPLE = 'crate::application::workflow::sample'


def code_only(source):
    # Production sample sources use ordinary Rust strings/comments. Fail closed on raw strings,
    # which need a separate lexer before being admitted here (avoids masking executable code).
    if re.search(r'\br#*"', source):
        raise ValueError('raw strings require boundary lexer review')
    return re.sub(r'//[^\n]*|/\*[\s\S]*?\*/|"(?:\\.|[^"\\])*"', ' ', source)


def expand_use(body):
    tokens = re.findall(r'::|[A-Za-z_][A-Za-z_0-9]*|[{},*]', body)
    def group(index, prefix):
        paths=[]
        while index<len(tokens) and tokens[index]!='}':
            path=list(prefix)
            while index<len(tokens) and tokens[index] not in ('{',',','}'):
                if tokens[index]!='::': path.append(tokens[index])
                index+=1
            if index<len(tokens) and tokens[index]=='{':
                nested,index=group(index+1,path);paths.extend(nested)
            else: paths.append('::'.join(path))
            if index<len(tokens) and tokens[index]==',':index+=1
        return paths,index+1
    if re.search(r'\bas\b',body): raise ValueError('import aliases require boundary review')
    return group(0,[])[0]


def normalize(path, module):
    parts=path.removeprefix('::').split('::')
    if parts[0]=='super':
        base=module.split('::')
        while parts and parts[0]=='super':base.pop();parts.pop(0)
        return '::'.join(base+parts)
    if parts[0]=='self':return module+'::'+'::'.join(parts[1:])
    return path


def permitted(path):
    if path==SAMPLE or path.startswith(SAMPLE+'::'):return True
    if path.startswith('crate::domain::workflow::'):return True
    if path in ('crate::app_error::AppError','crate::app_error::AppResult'):return True
    prefix='crate::application::workflow::'
    if path.startswith(prefix):return path[len(prefix):] in PURE
    if path.split('::')[0] in EXTERNAL:return True
    if path.startswith('std::collections::') or path in ('std::sync::Arc','std::fmt::Display','u64::from'):return True
    return False


def qualified_paths(code):
    # Rust paths may start with :: and contain arbitrary whitespace/comments between tokens.
    # Comments have already been stripped; matching tokens avoids a spelling-based bypass.
    tokens=re.findall(r'::|[A-Za-z_][A-Za-z_0-9]*|[^\s]',code)
    paths=[]
    index=0
    while index<len(tokens):
        rooted=tokens[index]=='::'
        start=index+1 if rooted else index
        if start>=len(tokens) or not re.fullmatch(r'[A-Za-z_][A-Za-z_0-9]*',tokens[start]):
            index+=1;continue
        parts=[tokens[start]]
        end=start+1
        while end+1<len(tokens) and tokens[end]=='::' and re.fullmatch(r'[A-Za-z_][A-Za-z_0-9]*',tokens[end+1]):
            parts.append(tokens[end+1]);end+=2
        # Associated paths on local/imported Rust types are resolved through checked imports.
        if rooted or (len(parts)>1 and parts[0][0].islower()):
            paths.append(('::' if rooted else '')+'::'.join(parts))
        index=end
    return paths


def violations(root):
    if not (root/'mod.rs').is_file():return [f'{root}: missing production sample module']
    failures=[]
    for file in sorted(root.rglob('*.rs')):
        if file.name=='tests.rs' or file.stem.endswith('_tests'):continue
        module=SAMPLE+'::'+'::'.join(file.relative_to(root).with_suffix('').parts)
        module=module.removesuffix('::mod')
        try:
            code=code_only(file.read_text())
            code=re.sub(r'#\[cfg\(test\)\]\s*mod\s+tests\s*;', '', code)
            if re.search(r'\b(?:unsafe|extern|async)\b|\b(?:include|env|option_env)!|\bmod\s+(?:tests|\w+_tests)\b|#\[path',code):
                raise ValueError('effect escape or unguarded test module')
            uses=re.findall(r'\buse\s+([^;]+);',code)
            paths=[path for body in uses for path in expand_use(body)]
            code=re.sub(r'\buse\s+[^;]+;', '', code)
            paths+=qualified_paths(code)
            for path in paths:
                # Local sibling modules may re-export inert sample types.
                if path.startswith(('types::','children::','dispatch::','tools::')):
                    path=SAMPLE+'::'+path
                path=normalize(path,module)
                if not permitted(path):failures.append(f'{file}: forbidden sample dependency {path}')
        except ValueError as error:failures.append(f'{file}: {error}')
    return failures


if __name__=='__main__':
    root=Path(sys.argv[1]) if len(sys.argv)>1 else Path(__file__).resolve().parents[1]/'src/application/workflow/sample'
    errors=violations(root)
    if errors:print('\n'.join(errors),file=sys.stderr);sys.exit(1)
    print('workflow sample capability boundary passed')
