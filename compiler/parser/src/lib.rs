//! # ailang-parser
//!
//! A hand-written recursive-descent parser that turns a [`Token`] stream
//! into an [`ailang_ast::Program`].
//!
//! ## Why hand-written recursive descent instead of `chumsky`/`lalrpop`?
//!
//! Both are excellent and remain the intended long-term direction (see
//! `ai.toml` / roadmap). For v0.1 a hand-written parser was chosen because:
//! it has zero macro/build-script surface area to debug, its error recovery
//! strategy is fully under our control (important for the "excellent
//! compiler diagnostics" goal), and it can be incrementally migrated
//! statement-by-statement to a parser-combinator backend later without
//! changing the AST it produces. The parser is structured as one function
//! per grammar rule specifically so that migration is mechanical.

use ailang_ast::*;
use ailang_diagnostics::{Diagnostic, DiagnosticBag, Span};
use ailang_lexer::{tokenize, SpannedToken, Token};

pub struct Parser {
    tokens: Vec<SpannedToken>,
    pos: usize,
    diagnostics: DiagnosticBag,
}

pub type ParseResult = (Option<Program>, DiagnosticBag);

/// Convenience entry point: lex + parse `source` in one call.
pub fn parse(source: &str) -> ParseResult {
    let (tokens, lex_diags) = tokenize(source);
    let mut parser = Parser::new(tokens);
    let program = parser.parse_program();
    let mut diags = lex_diags;
    diags.extend(parser.diagnostics);
    (Some(program), diags)
}

impl Parser {
    pub fn new(tokens: Vec<SpannedToken>) -> Self {
        Self {
            tokens,
            pos: 0,
            diagnostics: DiagnosticBag::new(),
        }
    }

    // ---------------------------------------------------------------
    // Token stream helpers
    // ---------------------------------------------------------------

    fn peek(&self) -> &Token {
        &self.tokens[self.pos].token
    }

    fn peek_span(&self) -> Span {
        self.tokens[self.pos].span
    }

    fn previous_span(&self) -> Span {
        self.tokens[self.pos.saturating_sub(1)].span
    }

    fn is_at_end(&self) -> bool {
        matches!(self.peek(), Token::Eof)
    }

    fn advance(&mut self) -> Token {
        let tok = self.tokens[self.pos].token.clone();
        if !self.is_at_end() {
            self.pos += 1;
        }
        tok
    }

    fn check(&self, expected: &Token) -> bool {
        std::mem::discriminant(self.peek()) == std::mem::discriminant(expected)
    }

    fn match_tok(&mut self, expected: &Token) -> bool {
        if self.check(expected) {
            self.advance();
            true
        } else {
            false
        }
    }

    fn expect(&mut self, expected: Token, what: &str) -> Span {
        if self.check(&expected) {
            let span = self.peek_span();
            self.advance();
            span
        } else {
            let span = self.peek_span();
            self.diagnostics.push(
                Diagnostic::error(
                    "P001",
                    format!("expected {what}, found {:?}", self.peek()),
                    span,
                )
                .with_label(format!("expected {what} here"))
                .with_help(format!("insert `{what}`")),
            );
            span
        }
    }

    fn synchronize(&mut self) {
        // Error-recovery: skip tokens until we're at a statement boundary
        // so a single mistake doesn't cascade into dozens of errors.
        self.advance();
        while !self.is_at_end() {
            if matches!(self.tokens[self.pos - 1].token, Token::Semicolon) {
                return;
            }
            match self.peek() {
                Token::Let
                | Token::Const
                | Token::Fn
                | Token::If
                | Token::While
                | Token::For
                | Token::Return
                | Token::Import
                | Token::Module
                | Token::Agent
                | Token::Model
                | Token::Tool
                | Token::Permission
                | Token::Memory
                | Token::Prompt
                | Token::Schema
                | Token::Workflow
                | Token::Parallel
                | Token::Team
                | Token::Knowledge
                | Token::Network
                | Token::Cluster
                | Token::On
                | Token::Reasoning
                | Token::Embedding
                | Token::Dataset
                | Token::Vector
                | Token::Pipeline
                | Token::Task
                | Token::Deploy
                | Token::RBrace => return,
                _ => {
                    self.advance();
                }
            }
        }
    }

    // ---------------------------------------------------------------
    // Program / statements
    // ---------------------------------------------------------------

    pub fn parse_program(&mut self) -> Program {
        let start = self.peek_span();
        let mut statements = Vec::new();
        while !self.is_at_end() {
            match self.declaration() {
                Some(stmt) => statements.push(stmt),
                None => self.synchronize(),
            }
        }
        let end = self.previous_span();
        Program {
            statements,
            span: start.merge(end),
        }
    }

    fn declaration(&mut self) -> Option<Stmt> {
        match self.peek() {
            Token::Let | Token::Const => self.var_decl(),
            Token::Fn => self.function_decl(),
            Token::Import => self.import_decl(),
            Token::Module => self.module_decl(),
            Token::Agent => self.agent_decl(),
            Token::Model => self.model_decl(),
            Token::Tool => self.tool_decl(),
            Token::Permission => self.permission_decl(),
            Token::Memory => self.memory_decl(),
            Token::Prompt => self.prompt_decl(),
            Token::Schema => self.schema_decl(),
            Token::Workflow => self.workflow_decl(),
            Token::Parallel => self.parallel_decl(),
            Token::Team => self.team_decl(),
            Token::Knowledge => self.knowledge_decl(),
            Token::Network => self.network_decl(),
            Token::Cluster => self.cluster_decl(),
            Token::On => self.on_event_decl(),
            Token::Reasoning
            | Token::Embedding
            | Token::Dataset
            | Token::Vector
            | Token::Pipeline
            | Token::Task
            | Token::Deploy => self.ai_placeholder_decl(),
            _ => self.statement(),
        }
    }

    fn var_decl(&mut self) -> Option<Stmt> {
        let start = self.peek_span();
        let is_const = matches!(self.peek(), Token::Const);
        self.advance(); // let/const

        let name = self.identifier("variable name")?;

        let type_annotation = if self.match_tok(&Token::Colon) {
            Some(self.parse_type()?)
        } else {
            None
        };

        let initializer = if self.match_tok(&Token::Eq) {
            Some(self.expression()?)
        } else {
            None
        };

        let end = self.expect(Token::Semicolon, "';'");
        Some(Stmt::VarDecl {
            name,
            is_const,
            type_annotation,
            initializer,
            span: start.merge(end),
        })
    }

    fn function_decl(&mut self) -> Option<Stmt> {
        let start = self.peek_span();
        self.advance(); // fn
        let name = self.identifier("function name")?;
        self.expect(Token::LParen, "'('");

        let mut params = Vec::new();
        if !self.check(&Token::RParen) {
            loop {
                let pstart = self.peek_span();
                let pname = self.identifier("parameter name")?;
                let type_annotation = if self.match_tok(&Token::Colon) {
                    Some(self.parse_type()?)
                } else {
                    None
                };
                let pend = self.previous_span();
                params.push(Param {
                    name: pname,
                    type_annotation,
                    span: pstart.merge(pend),
                });
                if !self.match_tok(&Token::Comma) {
                    break;
                }
            }
        }
        self.expect(Token::RParen, "')'");

        let return_type = if self.match_tok(&Token::Arrow) {
            Some(self.parse_type()?)
        } else {
            None
        };

        let body = self.block()?;
        let end = body.span;
        Some(Stmt::Function {
            name,
            params,
            return_type,
            body,
            span: start.merge(end),
        })
    }

    fn import_decl(&mut self) -> Option<Stmt> {
        let start = self.peek_span();
        self.advance(); // import
        let mut path = vec![self.identifier("module name")?];
        while self.match_tok(&Token::Dot) {
            path.push(self.identifier("module name")?);
        }
        let end = self.expect(Token::Semicolon, "';'");
        Some(Stmt::Import {
            path,
            span: start.merge(end),
        })
    }

    fn module_decl(&mut self) -> Option<Stmt> {
        let start = self.peek_span();
        self.advance(); // module
        let name = self.identifier("module name")?;
        self.expect(Token::LBrace, "'{'");
        let mut body = Vec::new();
        while !self.check(&Token::RBrace) && !self.is_at_end() {
            match self.declaration() {
                Some(s) => body.push(s),
                None => self.synchronize(),
            }
        }
        let end = self.expect(Token::RBrace, "'}'");
        Some(Stmt::Module {
            name,
            body,
            span: start.merge(end),
        })
    }

    // -----------------------------------------------------------------
    // v0.5 AI-native declarations
    // -----------------------------------------------------------------

    /// `agent Name { model M; memory persistent; tools [A, B]; permissions [internet]; }`
    fn agent_decl(&mut self) -> Option<Stmt> {
        let start = self.peek_span();
        self.advance(); // agent
        let name = self.identifier("agent name")?;

        // Optional `on <location>` clause (`agent Planner on cloud
        // { ... }`). `on` was a contextual identifier through v1.0; v2.0
        // promotes it to a real keyword (for `on <event> { }` handlers —
        // see `on_event_decl`), so this now matches `Token::On` directly.
        let location = if self.match_tok(&Token::On) {
            Some(self.identifier("location name")?)
        } else {
            None
        };

        self.expect(Token::LBrace, "'{'");

        let mut config = AgentConfig::default();
        while !self.check(&Token::RBrace) && !self.is_at_end() {
            match self.peek().clone() {
                Token::Model => {
                    self.advance();
                    config.model = Some(self.identifier("model name")?);
                    self.expect(Token::Semicolon, "';'");
                }
                Token::Memory => {
                    self.advance();
                    config.memory = Some(self.memory_ref()?);
                    self.expect(Token::Semicolon, "';'");
                }
                Token::Identifier(field) if field == "tools" => {
                    self.advance();
                    config.tools = self.identifier_list_in_brackets("tool")?;
                    self.expect(Token::Semicolon, "';'");
                }
                Token::Identifier(field) if field == "models" => {
                    self.advance();
                    config.models = self.identifier_list_in_brackets("model")?;
                    self.expect(Token::Semicolon, "';'");
                }
                Token::Identifier(field) if field == "permissions" => {
                    self.advance();
                    config.permissions = self.identifier_list_in_brackets("permission")?;
                    self.expect(Token::Semicolon, "';'");
                }
                _ => {
                    let span = self.peek_span();
                    self.diagnostics.push(Diagnostic::error(
                        "P010",
                        format!(
                            "unexpected token in `agent` body: {:?} (expected `model`, `models`, `memory`, `tools`, or `permissions`)",
                            self.peek()
                        ),
                        span,
                    ));
                    self.synchronize();
                }
            }
        }
        let end = self.expect(Token::RBrace, "'}'");
        Some(Stmt::Agent {
            name,
            location,
            config,
            span: start.merge(end),
        })
    }

    fn memory_ref(&mut self) -> Option<MemoryRef> {
        match self.peek().clone() {
            Token::Identifier(name) if name == "persistent" => {
                self.advance();
                Some(MemoryRef::Persistent)
            }
            Token::Identifier(name) if name == "temporary" => {
                self.advance();
                Some(MemoryRef::Temporary)
            }
            Token::Identifier(_) => Some(MemoryRef::Named(self.identifier("memory name")?)),
            _ => {
                let span = self.peek_span();
                self.diagnostics.push(Diagnostic::error(
                    "P011",
                    "expected `persistent`, `temporary`, or a memory name",
                    span,
                ));
                None
            }
        }
    }

    fn identifier_list_in_brackets(&mut self, what: &str) -> Option<Vec<Ident>> {
        self.expect(Token::LBracket, "'['");
        let mut items = Vec::new();
        if !self.check(&Token::RBracket) {
            loop {
                items.push(self.identifier(what)?);
                if !self.match_tok(&Token::Comma) {
                    break;
                }
            }
        }
        self.expect(Token::RBracket, "']'");
        Some(items)
    }

    /// `model Name;` or `model Name { endpoint "..."; key "value"; }`
    fn model_decl(&mut self) -> Option<Stmt> {
        let start = self.peek_span();
        self.advance(); // model
        let name = self.identifier("model name")?;

        let mut endpoint = None;
        let mut config = Vec::new();
        let end;
        if self.match_tok(&Token::LBrace) {
            while !self.check(&Token::RBrace) && !self.is_at_end() {
                let key = self.identifier("config key")?;
                let value = self.expression()?;
                self.expect(Token::Semicolon, "';'");
                if key.name == "endpoint" {
                    endpoint = Some(value);
                } else {
                    config.push((key, value));
                }
            }
            end = self.expect(Token::RBrace, "'}'");
        } else {
            end = self.expect(Token::Semicolon, "';'");
        }
        Some(Stmt::Model {
            name,
            endpoint,
            config,
            span: start.merge(end),
        })
    }

    /// `tool Name;`
    fn tool_decl(&mut self) -> Option<Stmt> {
        let start = self.peek_span();
        self.advance(); // tool
        let name = self.identifier("tool name")?;
        let end = self.expect(Token::Semicolon, "';'");
        Some(Stmt::Tool {
            name,
            span: start.merge(end),
        })
    }

    /// `permission internet;`
    fn permission_decl(&mut self) -> Option<Stmt> {
        let start = self.peek_span();
        self.advance(); // permission
        let name = self.identifier("permission name")?;
        let end = self.expect(Token::Semicolon, "';'");
        Some(Stmt::Permission {
            name,
            span: start.merge(end),
        })
    }

    /// `memory Name persistent;` / `memory Name temporary;`
    fn memory_decl(&mut self) -> Option<Stmt> {
        let start = self.peek_span();
        self.advance(); // memory
        let name = self.identifier("memory name")?;
        let kind = match self.peek().clone() {
            Token::Identifier(k) if k == "persistent" => {
                self.advance();
                MemoryKind::Persistent
            }
            Token::Identifier(k) if k == "temporary" => {
                self.advance();
                MemoryKind::Temporary
            }
            _ => {
                let span = self.peek_span();
                self.diagnostics.push(Diagnostic::error(
                    "P012",
                    "expected `persistent` or `temporary` after memory name",
                    span,
                ));
                return None;
            }
        };
        let end = self.expect(Token::Semicolon, "';'");
        Some(Stmt::Memory {
            name,
            kind,
            span: start.merge(end),
        })
    }

    /// `prompt Name = "text";` / `prompt Name = """multi\nline""";`
    fn prompt_decl(&mut self) -> Option<Stmt> {
        let start = self.peek_span();
        self.advance(); // prompt
        let name = self.identifier("prompt name")?;
        self.expect(Token::Eq, "'='");
        let template = self.expression()?;
        let end = self.expect(Token::Semicolon, "';'");
        Some(Stmt::Prompt {
            name,
            template,
            span: start.merge(end),
        })
    }

    /// `schema Name { field: Type; field: Type; }`
    fn schema_decl(&mut self) -> Option<Stmt> {
        let start = self.peek_span();
        self.advance(); // schema
        let name = self.identifier("schema name")?;
        self.expect(Token::LBrace, "'{'");
        let mut fields = Vec::new();
        while !self.check(&Token::RBrace) && !self.is_at_end() {
            let field_name = self.identifier("field name")?;
            self.expect(Token::Colon, "':'");
            let field_type = self.parse_type()?;
            self.expect(Token::Semicolon, "';'");
            fields.push((field_name, field_type));
        }
        let end = self.expect(Token::RBrace, "'}'");
        Some(Stmt::Schema {
            name,
            fields,
            span: start.merge(end),
        })
    }

    /// `workflow Name { Stage -> Stage -> Stage; }`
    fn workflow_decl(&mut self) -> Option<Stmt> {
        let start = self.peek_span();
        self.advance(); // workflow
        let name = self.identifier("workflow name")?;
        self.expect(Token::LBrace, "'{'");
        let mut stages = vec![self.identifier("workflow stage")?];
        while self.match_tok(&Token::Arrow) {
            stages.push(self.identifier("workflow stage")?);
        }
        self.expect(Token::Semicolon, "';'");
        let end = self.expect(Token::RBrace, "'}'");
        Some(Stmt::Workflow {
            name,
            stages,
            span: start.merge(end),
        })
    }

    /// `parallel { stmt; stmt; }`
    fn parallel_decl(&mut self) -> Option<Stmt> {
        let start = self.peek_span();
        self.advance(); // parallel
        let body = self.block()?;
        let end = body.span;
        Some(Stmt::Parallel {
            body,
            span: start.merge(end),
        })
    }

    /// `team Name { Member; Member; }`
    fn team_decl(&mut self) -> Option<Stmt> {
        let start = self.peek_span();
        self.advance(); // team
        let name = self.identifier("team name")?;
        self.expect(Token::LBrace, "'{'");
        let mut members = Vec::new();
        while !self.check(&Token::RBrace) && !self.is_at_end() {
            members.push(self.identifier("team member")?);
            self.expect(Token::Semicolon, "';'");
        }
        let end = self.expect(Token::RBrace, "'}'");
        Some(Stmt::Team {
            name,
            members,
            span: start.merge(end),
        })
    }

    /// `knowledge Name { source "./docs"; }`
    fn knowledge_decl(&mut self) -> Option<Stmt> {
        let start = self.peek_span();
        self.advance(); // knowledge
        let name = self.identifier("knowledge name")?;
        self.expect(Token::LBrace, "'{'");
        let source_key = self.identifier("`source`")?;
        if source_key.name != "source" {
            self.diagnostics.push(Diagnostic::error(
                "P013",
                format!("expected `source`, found `{}`", source_key.name),
                source_key.span,
            ));
        }
        let source = self.expression()?;
        self.expect(Token::Semicolon, "';'");
        let end = self.expect(Token::RBrace, "'}'");
        Some(Stmt::Knowledge {
            name,
            source,
            span: start.merge(end),
        })
    }

    /// Generic placeholder declaration for `reasoning`, `embedding`,
    /// `dataset`, `vector`, `pipeline`, `task`, `deploy`:
    /// `keyword Name;` or `keyword Name { key value; ... }`.
    fn ai_placeholder_decl(&mut self) -> Option<Stmt> {
        let start = self.peek_span();
        let keyword = match self.peek() {
            Token::Reasoning => AiPlaceholderKind::Reasoning,
            Token::Embedding => AiPlaceholderKind::Embedding,
            Token::Dataset => AiPlaceholderKind::Dataset,
            Token::Vector => AiPlaceholderKind::Vector,
            Token::Pipeline => AiPlaceholderKind::Pipeline,
            Token::Task => AiPlaceholderKind::Task,
            Token::Deploy => AiPlaceholderKind::Deploy,
            other => unreachable!("ai_placeholder_decl called on non-placeholder token {other:?}"),
        };
        self.advance();
        let name = self.identifier(&format!("{} name", keyword.keyword()))?;

        let mut config = Vec::new();
        let end;
        if self.match_tok(&Token::LBrace) {
            while !self.check(&Token::RBrace) && !self.is_at_end() {
                let key = self.identifier("config key")?;
                let value = self.expression()?;
                self.expect(Token::Semicolon, "';'");
                config.push((key, value));
            }
            end = self.expect(Token::RBrace, "'}'");
        } else {
            end = self.expect(Token::Semicolon, "';'");
        }
        Some(Stmt::AiPlaceholder {
            keyword,
            name,
            config,
            span: start.merge(end),
        })
    }

    /// `network Name { agent A; agent B; }`
    fn network_decl(&mut self) -> Option<Stmt> {
        let start = self.peek_span();
        self.advance(); // network
        let name = self.identifier("network name")?;
        self.expect(Token::LBrace, "'{'");
        let mut members = Vec::new();
        while !self.check(&Token::RBrace) && !self.is_at_end() {
            self.expect(Token::Agent, "'agent'");
            members.push(self.identifier("agent name")?);
            self.expect(Token::Semicolon, "';'");
        }
        let end = self.expect(Token::RBrace, "'}'");
        Some(Stmt::Network {
            name,
            members,
            span: start.merge(end),
        })
    }

    /// `cluster Name { replicas 100; autoscaling enabled; }`
    fn cluster_decl(&mut self) -> Option<Stmt> {
        let start = self.peek_span();
        self.advance(); // cluster
        let name = self.identifier("cluster name")?;
        self.expect(Token::LBrace, "'{'");

        let mut replicas = None;
        let mut autoscaling = None;
        while !self.check(&Token::RBrace) && !self.is_at_end() {
            let field = self.identifier("`replicas` or `autoscaling`")?;
            match field.name.as_str() {
                "replicas" => {
                    replicas = Some(self.expression()?);
                    self.expect(Token::Semicolon, "';'");
                }
                "autoscaling" => {
                    let state = self.identifier("`enabled` or `disabled`")?;
                    autoscaling = Some(match state.name.as_str() {
                        "enabled" => true,
                        "disabled" => false,
                        other => {
                            self.diagnostics.push(Diagnostic::error(
                                "P014",
                                format!("expected `enabled` or `disabled`, found `{other}`"),
                                state.span,
                            ));
                            false
                        }
                    });
                    self.expect(Token::Semicolon, "';'");
                }
                other => {
                    self.diagnostics.push(Diagnostic::error(
                        "P015",
                        format!("unexpected field `{other}` in `cluster` body (expected `replicas` or `autoscaling`)"),
                        field.span,
                    ));
                    self.synchronize();
                }
            }
        }
        let end = self.expect(Token::RBrace, "'}'");
        Some(Stmt::Cluster {
            name,
            replicas,
            autoscaling,
            span: start.merge(end),
        })
    }

    /// `on document_uploaded { ... }`
    fn on_event_decl(&mut self) -> Option<Stmt> {
        let start = self.peek_span();
        self.advance(); // on
        let event_name = self.identifier("event name")?;
        let body = self.block()?;
        let end = body.span;
        Some(Stmt::OnEvent {
            event_name,
            body,
            span: start.merge(end),
        })
    }

    fn statement(&mut self) -> Option<Stmt> {
        match self.peek() {
            Token::Return => self.return_stmt(),
            Token::If => self.if_stmt(),
            Token::While => self.while_stmt(),
            Token::For => self.for_stmt(),
            Token::LBrace => self.block().map(Stmt::Block),
            Token::Break => {
                let span = self.peek_span();
                self.advance();
                let end = self.expect(Token::Semicolon, "';'");
                Some(Stmt::Break {
                    span: span.merge(end),
                })
            }
            Token::Continue => {
                let span = self.peek_span();
                self.advance();
                let end = self.expect(Token::Semicolon, "';'");
                Some(Stmt::Continue {
                    span: span.merge(end),
                })
            }
            _ => self.expr_stmt(),
        }
    }

    fn return_stmt(&mut self) -> Option<Stmt> {
        let start = self.peek_span();
        self.advance(); // return
        let value = if !self.check(&Token::Semicolon) {
            Some(self.expression()?)
        } else {
            None
        };
        let end = self.expect(Token::Semicolon, "';'");
        Some(Stmt::Return {
            value,
            span: start.merge(end),
        })
    }

    fn if_stmt(&mut self) -> Option<Stmt> {
        let start = self.peek_span();
        self.advance(); // if
        let condition = self.expression()?;
        let then_branch = self.block()?;
        let else_branch = if self.match_tok(&Token::Else) {
            if self.check(&Token::If) {
                Some(Box::new(ElseBranch::ElseIf(Box::new(self.if_stmt()?))))
            } else {
                Some(Box::new(ElseBranch::Else(self.block()?)))
            }
        } else {
            None
        };
        let end = self.previous_span();
        Some(Stmt::If {
            condition,
            then_branch,
            else_branch,
            span: start.merge(end),
        })
    }

    fn while_stmt(&mut self) -> Option<Stmt> {
        let start = self.peek_span();
        self.advance(); // while
        let condition = self.expression()?;
        let body = self.block()?;
        let end = body.span;
        Some(Stmt::While {
            condition,
            body,
            span: start.merge(end),
        })
    }

    fn for_stmt(&mut self) -> Option<Stmt> {
        let start = self.peek_span();
        self.advance(); // for
        let variable = self.identifier("loop variable")?;
        self.expect(Token::In, "'in'");
        let range_start = self.expression()?;
        let inclusive = if self.match_tok(&Token::DotDotEq) {
            true
        } else {
            self.expect(Token::DotDot, "'..'");
            false
        };
        let range_end = self.expression()?;
        let body = self.block()?;
        let end = body.span;
        Some(Stmt::For {
            variable,
            start: range_start,
            end: range_end,
            inclusive,
            body,
            span: start.merge(end),
        })
    }

    fn block(&mut self) -> Option<Block> {
        let start = self.expect(Token::LBrace, "'{'");
        let mut statements = Vec::new();
        while !self.check(&Token::RBrace) && !self.is_at_end() {
            match self.declaration() {
                Some(s) => statements.push(s),
                None => self.synchronize(),
            }
        }
        let end = self.expect(Token::RBrace, "'}'");
        Some(Block {
            statements,
            span: start.merge(end),
        })
    }

    fn expr_stmt(&mut self) -> Option<Stmt> {
        let expr = self.expression()?;
        let end = self.expect(Token::Semicolon, "';'");
        let span = expr.span().merge(end);
        Some(Stmt::Expression { expr, span })
    }

    // ---------------------------------------------------------------
    // Types
    // ---------------------------------------------------------------

    fn parse_type(&mut self) -> Option<TypeExpr> {
        match self.peek().clone() {
            Token::IntType => {
                self.advance();
                Some(TypeExpr::Int)
            }
            Token::FloatType => {
                self.advance();
                Some(TypeExpr::Float)
            }
            Token::BoolType => {
                self.advance();
                Some(TypeExpr::Bool)
            }
            Token::StringType => {
                self.advance();
                Some(TypeExpr::String)
            }
            Token::CharType => {
                self.advance();
                Some(TypeExpr::Char)
            }
            Token::Null => {
                self.advance();
                Some(TypeExpr::Null)
            }
            Token::Void => {
                self.advance();
                Some(TypeExpr::Void)
            }
            Token::LBracket => {
                self.advance();
                let inner = self.parse_type()?;
                self.expect(Token::RBracket, "']'");
                Some(TypeExpr::Array(Box::new(inner)))
            }
            Token::LBrace => {
                self.advance();
                let key = self.parse_type()?;
                self.expect(Token::Colon, "':'");
                let value = self.parse_type()?;
                self.expect(Token::RBrace, "'}'");
                Some(TypeExpr::Map(Box::new(key), Box::new(value)))
            }
            Token::Fn => {
                self.advance();
                self.expect(Token::LParen, "'('");
                let mut params = Vec::new();
                if !self.check(&Token::RParen) {
                    loop {
                        params.push(self.parse_type()?);
                        if !self.match_tok(&Token::Comma) {
                            break;
                        }
                    }
                }
                self.expect(Token::RParen, "')'");
                self.expect(Token::Arrow, "'->'");
                let ret = self.parse_type()?;
                Some(TypeExpr::Function {
                    params,
                    ret: Box::new(ret),
                })
            }
            Token::Identifier(name) => {
                self.advance();
                Some(TypeExpr::Named(name))
            }
            _ => {
                let span = self.peek_span();
                self.diagnostics.push(Diagnostic::error(
                    "P002",
                    format!("expected a type, found {:?}", self.peek()),
                    span,
                ));
                None
            }
        }
    }

    fn identifier(&mut self, what: &str) -> Option<Ident> {
        if let Token::Identifier(name) = self.peek().clone() {
            let span = self.peek_span();
            self.advance();
            Some(Ident::new(name, span))
        } else {
            let span = self.peek_span();
            self.diagnostics.push(Diagnostic::error(
                "P003",
                format!("expected {what}, found {:?}", self.peek()),
                span,
            ));
            None
        }
    }

    // ---------------------------------------------------------------
    // Expressions (precedence climbing)
    // ---------------------------------------------------------------

    fn expression(&mut self) -> Option<Expr> {
        self.assignment()
    }

    fn assignment(&mut self) -> Option<Expr> {
        let expr = self.logic_or()?;

        let op = match self.peek() {
            Token::Eq => Some(AssignOp::Assign),
            Token::PlusEq => Some(AssignOp::AddAssign),
            Token::MinusEq => Some(AssignOp::SubAssign),
            Token::StarEq => Some(AssignOp::MulAssign),
            Token::SlashEq => Some(AssignOp::DivAssign),
            _ => None,
        };

        if let Some(op) = op {
            let op_span = self.peek_span();
            self.advance();
            let value = self.assignment()?;
            let span = expr.span().merge(value.span());
            return match &expr {
                Expr::Identifier { .. } | Expr::Index { .. } | Expr::Member { .. } => {
                    Some(Expr::Assign {
                        target: Box::new(expr),
                        op,
                        value: Box::new(value),
                        span,
                    })
                }
                _ => {
                    self.diagnostics.push(Diagnostic::error(
                        "P004",
                        "invalid assignment target",
                        op_span,
                    ));
                    None
                }
            };
        }

        Some(expr)
    }

    fn logic_or(&mut self) -> Option<Expr> {
        let mut expr = self.logic_and()?;
        while self.check(&Token::OrOr) {
            self.advance();
            let right = self.logic_and()?;
            let span = expr.span().merge(right.span());
            expr = Expr::Binary {
                left: Box::new(expr),
                op: BinaryOp::Or,
                right: Box::new(right),
                span,
            };
        }
        Some(expr)
    }

    fn logic_and(&mut self) -> Option<Expr> {
        let mut expr = self.equality()?;
        while self.check(&Token::AndAnd) {
            self.advance();
            let right = self.equality()?;
            let span = expr.span().merge(right.span());
            expr = Expr::Binary {
                left: Box::new(expr),
                op: BinaryOp::And,
                right: Box::new(right),
                span,
            };
        }
        Some(expr)
    }

    fn equality(&mut self) -> Option<Expr> {
        let mut expr = self.comparison()?;
        loop {
            let op = match self.peek() {
                Token::EqEq => BinaryOp::Eq,
                Token::NotEq => BinaryOp::NotEq,
                _ => break,
            };
            self.advance();
            let right = self.comparison()?;
            let span = expr.span().merge(right.span());
            expr = Expr::Binary {
                left: Box::new(expr),
                op,
                right: Box::new(right),
                span,
            };
        }
        Some(expr)
    }

    fn comparison(&mut self) -> Option<Expr> {
        let mut expr = self.term()?;
        loop {
            let op = match self.peek() {
                Token::Gt => BinaryOp::Gt,
                Token::Lt => BinaryOp::Lt,
                Token::GtEq => BinaryOp::GtEq,
                Token::LtEq => BinaryOp::LtEq,
                _ => break,
            };
            self.advance();
            let right = self.term()?;
            let span = expr.span().merge(right.span());
            expr = Expr::Binary {
                left: Box::new(expr),
                op,
                right: Box::new(right),
                span,
            };
        }
        Some(expr)
    }

    fn term(&mut self) -> Option<Expr> {
        let mut expr = self.factor()?;
        loop {
            let op = match self.peek() {
                Token::Plus => BinaryOp::Add,
                Token::Minus => BinaryOp::Sub,
                _ => break,
            };
            self.advance();
            let right = self.factor()?;
            let span = expr.span().merge(right.span());
            expr = Expr::Binary {
                left: Box::new(expr),
                op,
                right: Box::new(right),
                span,
            };
        }
        Some(expr)
    }

    fn factor(&mut self) -> Option<Expr> {
        let mut expr = self.unary()?;
        loop {
            let op = match self.peek() {
                Token::Star => BinaryOp::Mul,
                Token::Slash => BinaryOp::Div,
                Token::Percent => BinaryOp::Mod,
                _ => break,
            };
            self.advance();
            let right = self.unary()?;
            let span = expr.span().merge(right.span());
            expr = Expr::Binary {
                left: Box::new(expr),
                op,
                right: Box::new(right),
                span,
            };
        }
        Some(expr)
    }

    fn unary(&mut self) -> Option<Expr> {
        let op = match self.peek() {
            Token::Minus => Some(UnaryOp::Neg),
            Token::Bang => Some(UnaryOp::Not),
            _ => None,
        };
        if let Some(op) = op {
            let start = self.peek_span();
            self.advance();
            let operand = self.unary()?;
            let span = start.merge(operand.span());
            return Some(Expr::Unary {
                op,
                operand: Box::new(operand),
                span,
            });
        }
        self.call()
    }

    fn call(&mut self) -> Option<Expr> {
        let mut expr = self.primary()?;
        loop {
            match self.peek() {
                Token::LParen => {
                    self.advance();
                    let mut args = Vec::new();
                    if !self.check(&Token::RParen) {
                        loop {
                            args.push(self.expression()?);
                            if !self.match_tok(&Token::Comma) {
                                break;
                            }
                        }
                    }
                    let end = self.expect(Token::RParen, "')'");
                    let span = expr.span().merge(end);
                    expr = Expr::Call {
                        callee: Box::new(expr),
                        args,
                        span,
                    };
                }
                Token::LBracket => {
                    self.advance();
                    let index = self.expression()?;
                    let end = self.expect(Token::RBracket, "']'");
                    let span = expr.span().merge(end);
                    expr = Expr::Index {
                        object: Box::new(expr),
                        index: Box::new(index),
                        span,
                    };
                }
                Token::Dot => {
                    self.advance();
                    let property = self.identifier("property name")?;
                    let span = expr.span().merge(property.span);
                    expr = Expr::Member {
                        object: Box::new(expr),
                        property,
                        span,
                    };
                }
                _ => break,
            }
        }
        Some(expr)
    }

    fn primary(&mut self) -> Option<Expr> {
        let span = self.peek_span();
        match self.peek().clone() {
            Token::IntLiteral(n) => {
                self.advance();
                Some(Expr::Literal {
                    value: Literal::Int(n),
                    span,
                })
            }
            Token::FloatLiteral(n) => {
                self.advance();
                Some(Expr::Literal {
                    value: Literal::Float(n),
                    span,
                })
            }
            Token::StringLiteral(s) => {
                self.advance();
                Some(Expr::Literal {
                    value: Literal::String(s),
                    span,
                })
            }
            Token::TripleQuotedString(s) => {
                self.advance();
                Some(Expr::Literal {
                    value: Literal::String(s),
                    span,
                })
            }
            Token::CharLiteral(c) => {
                self.advance();
                Some(Expr::Literal {
                    value: Literal::Char(c),
                    span,
                })
            }
            Token::True => {
                self.advance();
                Some(Expr::Literal {
                    value: Literal::Bool(true),
                    span,
                })
            }
            Token::False => {
                self.advance();
                Some(Expr::Literal {
                    value: Literal::Bool(false),
                    span,
                })
            }
            Token::Null => {
                self.advance();
                Some(Expr::Literal {
                    value: Literal::Null,
                    span,
                })
            }
            Token::Generate => {
                self.advance();
                let schema_name = self.identifier("schema name")?;
                let end = schema_name.span;
                Some(Expr::Generate {
                    schema_name,
                    span: span.merge(end),
                })
            }
            Token::Identifier(name) => {
                self.advance();
                Some(Expr::Identifier { name, span })
            }
            Token::LParen => {
                self.advance();
                let inner = self.expression()?;
                let end = self.expect(Token::RParen, "')'");
                Some(Expr::Grouping {
                    expr: Box::new(inner),
                    span: span.merge(end),
                })
            }
            Token::LBracket => {
                self.advance();
                let mut elements = Vec::new();
                if !self.check(&Token::RBracket) {
                    loop {
                        elements.push(self.expression()?);
                        if !self.match_tok(&Token::Comma) {
                            break;
                        }
                    }
                }
                let end = self.expect(Token::RBracket, "']'");
                Some(Expr::ArrayLiteral {
                    elements,
                    span: span.merge(end),
                })
            }
            Token::LBrace => {
                self.advance();
                let mut entries = Vec::new();
                if !self.check(&Token::RBrace) {
                    loop {
                        let key = self.expression()?;
                        self.expect(Token::Colon, "':'");
                        let value = self.expression()?;
                        entries.push((key, value));
                        if !self.match_tok(&Token::Comma) {
                            break;
                        }
                    }
                }
                let end = self.expect(Token::RBrace, "'}'");
                Some(Expr::MapLiteral {
                    entries,
                    span: span.merge(end),
                })
            }
            other => {
                self.diagnostics.push(
                    Diagnostic::error("P005", format!("unexpected token {other:?}"), span)
                        .with_label("expected an expression here"),
                );
                self.advance();
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_ok(src: &str) -> Program {
        let (program, diags) = parse(src);
        assert!(
            !diags.has_errors(),
            "unexpected errors: {:?}",
            diags.diagnostics
        );
        program.unwrap()
    }

    #[test]
    fn parses_hello_world() {
        let program = parse_ok(r#"print("Hello, world!");"#);
        assert_eq!(program.statements.len(), 1);
    }

    #[test]
    fn parses_let_and_const() {
        let program = parse_ok("let x = 10;\nconst PI = 3.14159;");
        assert_eq!(program.statements.len(), 2);
        match &program.statements[1] {
            Stmt::VarDecl { is_const, .. } => assert!(is_const),
            _ => panic!("expected VarDecl"),
        }
    }

    #[test]
    fn parses_function_with_return_type() {
        let program = parse_ok("fn add(a: int, b: int) -> int { return a + b; }");
        match &program.statements[0] {
            Stmt::Function {
                params,
                return_type,
                ..
            } => {
                assert_eq!(params.len(), 2);
                assert_eq!(return_type, &Some(TypeExpr::Int));
            }
            _ => panic!("expected Function"),
        }
    }

    #[test]
    fn parses_if_else_if_else() {
        let program = parse_ok(
            "if age >= 18 { print(\"Adult\"); } else if age >= 13 { print(\"Teen\"); } else { print(\"Kid\"); }",
        );
        assert_eq!(program.statements.len(), 1);
    }

    #[test]
    fn parses_for_range() {
        let program = parse_ok("for i in 0..10 { print(i); }");
        match &program.statements[0] {
            Stmt::For { inclusive, .. } => assert!(!inclusive),
            _ => panic!("expected For"),
        }
    }

    #[test]
    fn parses_arrays_and_maps() {
        let program = parse_ok(r#"let numbers = [1,2,3]; let ages = {"Alice":20,"Bob":25};"#);
        assert_eq!(program.statements.len(), 2);
    }

    #[test]
    fn parses_operator_precedence() {
        // 1 + 2 * 3 should parse as 1 + (2 * 3)
        let program = parse_ok("let x = 1 + 2 * 3;");
        if let Stmt::VarDecl {
            initializer: Some(Expr::Binary { op, right, .. }),
            ..
        } = &program.statements[0]
        {
            assert_eq!(*op, BinaryOp::Add);
            assert!(matches!(
                **right,
                Expr::Binary {
                    op: BinaryOp::Mul,
                    ..
                }
            ));
        } else {
            panic!("unexpected shape");
        }
    }

    #[test]
    fn parses_import_and_module() {
        let program =
            parse_ok("import std.io;\nmodule math { fn square(x: int) -> int { return x * x; } }");
        assert_eq!(program.statements.len(), 2);
    }

    #[test]
    fn reports_missing_semicolon() {
        let (_, diags) = parse("let x = 10");
        assert!(diags.has_errors());
    }

    #[test]
    fn parses_compound_assignment() {
        let program = parse_ok("let x = 1; x += 2;");
        match &program.statements[1] {
            Stmt::Expression {
                expr: Expr::Assign { op, .. },
                ..
            } => {
                assert_eq!(*op, AssignOp::AddAssign);
            }
            _ => panic!("expected assignment expression statement"),
        }
    }

    // -----------------------------------------------------------------
    // v0.5 AI-native construct parsing
    // -----------------------------------------------------------------

    #[test]
    fn parses_agent_with_full_config() {
        let program = parse_ok(
            r#"
            agent Researcher {
                model GPT5;
                memory persistent;
                tools [Search, Browser];
                permissions [internet];
            }
            "#,
        );
        match &program.statements[0] {
            Stmt::Agent { name, config, .. } => {
                assert_eq!(name.name, "Researcher");
                assert_eq!(config.model.as_ref().unwrap().name, "GPT5");
                assert_eq!(config.memory, Some(MemoryRef::Persistent));
                assert_eq!(config.tools.len(), 2);
                assert_eq!(config.permissions.len(), 1);
            }
            _ => panic!("expected Agent"),
        }
    }

    #[test]
    fn parses_agent_with_named_memory() {
        let program = parse_ok("agent A { memory UserMemory; }");
        match &program.statements[0] {
            Stmt::Agent { config, .. } => match &config.memory {
                Some(MemoryRef::Named(ident)) => assert_eq!(ident.name, "UserMemory"),
                other => panic!("expected MemoryRef::Named, got {other:?}"),
            },
            _ => panic!("expected Agent"),
        }
    }

    #[test]
    fn parses_bare_model_declaration() {
        let program = parse_ok("model GPT5;");
        match &program.statements[0] {
            Stmt::Model {
                name,
                endpoint,
                config,
                ..
            } => {
                assert_eq!(name.name, "GPT5");
                assert!(endpoint.is_none());
                assert!(config.is_empty());
            }
            _ => panic!("expected Model"),
        }
    }

    #[test]
    fn parses_model_with_endpoint_block() {
        let program = parse_ok(r#"model LocalModel { endpoint "http://localhost:11434"; }"#);
        match &program.statements[0] {
            Stmt::Model { name, endpoint, .. } => {
                assert_eq!(name.name, "LocalModel");
                assert!(endpoint.is_some());
            }
            _ => panic!("expected Model"),
        }
    }

    #[test]
    fn parses_tool_and_permission_declarations() {
        let program = parse_ok("tool Search; permission internet;");
        assert!(matches!(program.statements[0], Stmt::Tool { .. }));
        assert!(matches!(program.statements[1], Stmt::Permission { .. }));
    }

    #[test]
    fn parses_memory_declaration() {
        let program = parse_ok("memory UserMemory persistent; memory SessionMemory temporary;");
        match &program.statements[0] {
            Stmt::Memory { kind, .. } => assert_eq!(*kind, MemoryKind::Persistent),
            _ => panic!("expected Memory"),
        }
        match &program.statements[1] {
            Stmt::Memory { kind, .. } => assert_eq!(*kind, MemoryKind::Temporary),
            _ => panic!("expected Memory"),
        }
    }

    #[test]
    fn parses_prompt_with_triple_quoted_template() {
        let program = parse_ok("prompt system = \"\"\"\nYou are an expert researcher.\n\"\"\";");
        match &program.statements[0] {
            Stmt::Prompt { name, template, .. } => {
                assert_eq!(name.name, "system");
                assert!(matches!(
                    template,
                    Expr::Literal {
                        value: Literal::String(_),
                        ..
                    }
                ));
            }
            _ => panic!("expected Prompt"),
        }
    }

    #[test]
    fn parses_schema_declaration() {
        let program = parse_ok("schema Person { name: string; age: int; }");
        match &program.statements[0] {
            Stmt::Schema { name, fields, .. } => {
                assert_eq!(name.name, "Person");
                assert_eq!(fields.len(), 2);
                assert_eq!(fields[0].0.name, "name");
                assert_eq!(fields[0].1, TypeExpr::String);
                assert_eq!(fields[1].1, TypeExpr::Int);
            }
            _ => panic!("expected Schema"),
        }
    }

    #[test]
    fn parses_generate_expression() {
        let program = parse_ok("let p = generate Person;");
        match &program.statements[0] {
            Stmt::VarDecl {
                initializer: Some(Expr::Generate { schema_name, .. }),
                ..
            } => {
                assert_eq!(schema_name.name, "Person");
            }
            _ => panic!("expected VarDecl with Generate initializer"),
        }
    }

    #[test]
    fn parses_workflow_chain() {
        let program = parse_ok("workflow Paper { Researcher -> Writer -> Reviewer; }");
        match &program.statements[0] {
            Stmt::Workflow { name, stages, .. } => {
                assert_eq!(name.name, "Paper");
                let names: Vec<&str> = stages.iter().map(|i| i.name.as_str()).collect();
                assert_eq!(names, vec!["Researcher", "Writer", "Reviewer"]);
            }
            _ => panic!("expected Workflow"),
        }
    }

    #[test]
    fn parses_parallel_block() {
        let program = parse_ok("parallel { print(1); print(2); }");
        match &program.statements[0] {
            Stmt::Parallel { body, .. } => assert_eq!(body.statements.len(), 2),
            _ => panic!("expected Parallel"),
        }
    }

    #[test]
    fn parses_team_declaration() {
        let program = parse_ok("team Startup { CEO; CTO; Designer; }");
        match &program.statements[0] {
            Stmt::Team { name, members, .. } => {
                assert_eq!(name.name, "Startup");
                assert_eq!(members.len(), 3);
            }
            _ => panic!("expected Team"),
        }
    }

    #[test]
    fn parses_knowledge_declaration() {
        let program = parse_ok(r#"knowledge Docs { source "./docs"; }"#);
        match &program.statements[0] {
            Stmt::Knowledge { name, .. } => assert_eq!(name.name, "Docs"),
            _ => panic!("expected Knowledge"),
        }
    }

    #[test]
    fn parses_all_ai_placeholder_keywords() {
        let program = parse_ok(
            "reasoning R; embedding E; dataset D; vector V; pipeline P; task T; deploy Dep;",
        );
        assert_eq!(program.statements.len(), 7);
        let expected = [
            AiPlaceholderKind::Reasoning,
            AiPlaceholderKind::Embedding,
            AiPlaceholderKind::Dataset,
            AiPlaceholderKind::Vector,
            AiPlaceholderKind::Pipeline,
            AiPlaceholderKind::Task,
            AiPlaceholderKind::Deploy,
        ];
        for (stmt, expected_kind) in program.statements.iter().zip(expected.iter()) {
            match stmt {
                Stmt::AiPlaceholder { keyword, .. } => assert_eq!(keyword, expected_kind),
                _ => panic!("expected AiPlaceholder"),
            }
        }
    }

    #[test]
    fn parses_ai_placeholder_with_config_block() {
        let program = parse_ok(r#"dataset Reviews { source "./reviews.csv"; }"#);
        match &program.statements[0] {
            Stmt::AiPlaceholder {
                keyword,
                name,
                config,
                ..
            } => {
                assert_eq!(*keyword, AiPlaceholderKind::Dataset);
                assert_eq!(name.name, "Reviews");
                assert_eq!(config.len(), 1);
            }
            _ => panic!("expected AiPlaceholder"),
        }
    }

    #[test]
    fn v05_constructs_coexist_with_v01_program() {
        // A full "hello agent" style program mixing traditional and
        // AI-native constructs, to guard against any grammar interaction
        // bugs between the two.
        let program = parse_ok(
            r#"
            fn greet(name: string) -> string {
                return "Hello, " + name;
            }

            model GPT5;
            tool Search;
            permission internet;

            agent Researcher {
                model GPT5;
                tools [Search];
                permissions [internet];
            }

            println(greet("world"));
            "#,
        );
        assert_eq!(program.statements.len(), 6);
    }

    // -----------------------------------------------------------------
    // v2.0 distributed-intelligence construct parsing
    // -----------------------------------------------------------------

    #[test]
    fn parses_network_declaration() {
        let program = parse_ok(
            "agent Planner {} agent Researcher {} network Enterprise { agent Planner; agent Researcher; }",
        );
        match &program.statements[2] {
            Stmt::Network { name, members, .. } => {
                assert_eq!(name.name, "Enterprise");
                assert_eq!(members.len(), 2);
                assert_eq!(members[0].name, "Planner");
            }
            _ => panic!("expected Network"),
        }
    }

    #[test]
    fn parses_cluster_declaration() {
        let program = parse_ok("cluster AIWorkers { replicas 100; autoscaling enabled; }");
        match &program.statements[0] {
            Stmt::Cluster {
                name,
                replicas,
                autoscaling,
                ..
            } => {
                assert_eq!(name.name, "AIWorkers");
                assert!(replicas.is_some());
                assert_eq!(*autoscaling, Some(true));
            }
            _ => panic!("expected Cluster"),
        }
    }

    #[test]
    fn parses_cluster_with_autoscaling_disabled() {
        let program = parse_ok("cluster Small { replicas 2; autoscaling disabled; }");
        match &program.statements[0] {
            Stmt::Cluster { autoscaling, .. } => assert_eq!(*autoscaling, Some(false)),
            _ => panic!("expected Cluster"),
        }
    }

    #[test]
    fn parses_on_event_declaration() {
        let program = parse_ok("on document_uploaded { println(\"got one\"); }");
        match &program.statements[0] {
            Stmt::OnEvent {
                event_name, body, ..
            } => {
                assert_eq!(event_name.name, "document_uploaded");
                assert_eq!(body.statements.len(), 1);
            }
            _ => panic!("expected OnEvent"),
        }
    }

    #[test]
    fn parses_multiple_on_event_handlers_for_different_events() {
        let program =
            parse_ok("on task_completed { println(1); } on memory_updated { println(2); }");
        assert_eq!(program.statements.len(), 2);
    }

    #[test]
    fn parses_agent_with_multiple_models() {
        let program =
            parse_ok("model GPT5; model Claude; agent Analyst { models [GPT5, Claude]; }");
        match &program.statements[2] {
            Stmt::Agent { config, .. } => {
                assert_eq!(config.models.len(), 2);
                assert_eq!(config.models[0].name, "GPT5");
                assert_eq!(config.models[1].name, "Claude");
            }
            _ => panic!("expected Agent"),
        }
    }

    #[test]
    fn agent_on_location_still_parses_after_on_became_a_keyword() {
        // Regression test: promoting `on` to a real keyword (for `on
        // <event> { }`) must not break the existing v1.0 `agent Name on
        // location { }` syntax.
        let program = parse_ok("agent Planner on cloud { model GPT5; }");
        match &program.statements[0] {
            Stmt::Agent { location, .. } => {
                assert_eq!(location.as_ref().map(|l| l.name.as_str()), Some("cloud"));
            }
            _ => panic!("expected Agent"),
        }
    }

    #[test]
    fn full_v20_program_parses_cleanly() {
        let program = parse_ok(
            r#"
            model GPT5;
            agent Planner on cloud { model GPT5; }
            agent Researcher on edge { model GPT5; }
            network Enterprise { agent Planner; agent Researcher; }
            cluster Workers { replicas 10; autoscaling enabled; }
            on task_completed { println("done"); }
            "#,
        );
        assert_eq!(program.statements.len(), 6);
    }
}
