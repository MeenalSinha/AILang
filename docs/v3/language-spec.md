# AILang v3.0 Formal Language Specification

## 1. Lexical Grammar

Identifiers follow `[a-zA-Z_][a-zA-Z0-9_]*`.
Keywords are reserved and cannot be used as variable names.

### Reserved Keyword Additions in v3.0:
- Goals: `goal`, `plan`, `constraint`, `optimize`, `budget`
- Supervision: `supervisor`, `one_for_one`, `one_for_all`, `rest_for_one`
- Resource Management: `resource`, `intelligence`, `allocate`
- Agent Attributes: `autonomous`, `persistent`, `restart_on_failure`
- Reasoning: `reason`, `verify`, `revise`
- Devices: `device`, `camera`, `microphone`, `speaker`, `sensor`
- Interface: `ui`, `panel`, `dashboard`
- Events: `event`, `emit`, `when`, `on`
- Applications: `application`, `policy`, `allow`, `deny`
- Multimodal: `multimodal`, `image`, `audio`, `video`
- Lifecycle: `migrate`, `checkpoint`, `recover`

## 2. Statements Grammar (BNF fragment)

```ebnf
Statement ::= VarDecl
            | FunctionDecl
            | AgentDecl
            | ModelDecl
            | GoalDecl
            | SupervisorDecl
            | ResourceDecl
            | ReasonStmt
            | DeviceDecl
            | UiDecl
            | ApplicationDecl
            | PolicyDecl
            | EmitStmt
            | OnEventStmt
            | ExpressionStmt ;

GoalDecl ::= "goal" Identifier StringLiteral "{" [ "constraints" "{" { Constraint } "}" ] Block "}" ;

SupervisorDecl ::= "supervisor" Identifier ( "one_for_one" | "one_for_all" | "rest_for_one" )
                   "{" "children" "[" [ IdentifierList ] "]" ";" "}" ;

ResourceDecl ::= "resource" Identifier Identifier "{" { Identifier Expression ";" } "}" ;

ReasonStmt ::= "reason" [ "max" Expression "iterations" ] [ "budget" Expression ] Block ;

DeviceDecl ::= "device" Identifier ( "camera" | "microphone" | "speaker" | "sensor" | "gpu" )
               "{" [ "permissions" "[" [ StringList ] "]" ";" ] "}" ;

UiDecl ::= "ui" Identifier "{" { UiItem } "}" ;
UiItem ::= "title" StringLiteral ";" | "panel" Identifier Identifier ";" ;

ApplicationDecl ::= "application" Identifier "{" { AppItem } "}" ;

PolicyDecl ::= "policy" Identifier "{" { PolicyRule } "}" ;
PolicyRule ::= ( "allow" | "deny" ) ( Identifier | "*" ) ( Identifier | "*" ) ";" ;

EmitStmt ::= "emit" StringLiteral [ Expression ] ";" ;
```

