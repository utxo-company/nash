use std::collections::BTreeMap;

use bumpalo::Bump;
use nash_ast::{
    Annotation, CaseBranch as CanCaseBranch, ConstructorName, CtorOpts, Def as CanDef,
    Expr as CanExpr, FieldUpdate as CanFieldUpdate, FieldValue as CanFieldValue, FreeVars,
    IfBranch as CanIfBranch, ModuleName, QualifiedName, Type as CanType,
    TypedPattern as CanTypedPattern,
};
use nash_region::{Located, Region};
use nash_source::{
    BinOpOperand, CaseArm, Def as SourceDef, Expr as SourceExpr, FieldAssign,
    IfBranch as SourceIfBranch, VarType,
};

use crate::Error;
use crate::environment::{self, Ctor as EnvCtor, Env, Info, Var};
use crate::error::DuplicatePatternContext;
use crate::pattern::{self, Bindings};
use crate::scc;
use crate::types;
use crate::warning::{Warning, WarningContext};

pub type FreeLocals<'a> = BTreeMap<&'a str, Uses>;

#[derive(Clone, Copy, Debug)]
pub struct Uses {
    pub direct: u32,
    pub delayed: u32,
}

fn log_var<'a>(free_locals: &mut FreeLocals<'a>, name: &'a str) {
    free_locals
        .entry(name)
        .and_modify(|u| u.direct += 1)
        .or_insert(Uses {
            direct: 1,
            delayed: 0,
        });
}

fn delay_use(uses: Uses) -> Uses {
    Uses {
        direct: 0,
        delayed: uses.direct + uses.delayed,
    }
}

fn merge_free_locals<'a>(target: &mut FreeLocals<'a>, source: FreeLocals<'a>, delay: bool) {
    for (name, uses) in source {
        let uses = if delay { delay_use(uses) } else { uses };
        target
            .entry(name)
            .and_modify(|u| {
                u.direct += uses.direct;
                u.delayed += uses.delayed;
            })
            .or_insert(uses);
    }
}

pub fn verify_bindings<'a>(
    context: WarningContext,
    bindings: &Bindings<'a>,
    body_free_locals: FreeLocals<'a>,
    warnings: &mut Vec<Warning<'a>>,
) -> FreeLocals<'a> {
    let mut outer_free = FreeLocals::new();

    for (name, uses) in &body_free_locals {
        if !bindings.contains_key(name) {
            outer_free.insert(name, *uses);
        }
    }

    for (&name, &region) in bindings {
        if !name.starts_with('_') && !body_free_locals.contains_key(name) {
            warnings.push(Warning::UnusedVariable {
                region,
                context,
                name,
            });
        }
    }

    outer_free
}

pub fn canonicalize_expr<'a>(
    bump: &'a Bump,
    env: &Env<'a>,
    expr: &'a Located<SourceExpr<'a>>,
    free_locals: &mut FreeLocals<'a>,
    warnings: &mut Vec<Warning<'a>>,
) -> Result<&'a Located<CanExpr<'a>>, Vec<Error<'a>>> {
    let region = expr.region;
    let can_expr = match &expr.value {
        SourceExpr::Str(s) => CanExpr::Str(s),
        SourceExpr::Int(n) => CanExpr::Int(*n),

        SourceExpr::Var {
            kind: VarType::LowVar,
            name,
        } => find_var(bump, env, region, name, free_locals)?,

        SourceExpr::Var {
            kind: VarType::CapVar,
            name,
        } => {
            let ctor = env.find_ctor(bump, region, name)?;
            to_var_ctor(bump, name, &ctor)
        }

        SourceExpr::VarQual {
            kind: VarType::LowVar,
            module,
            name,
        } => find_var_qual(bump, env, region, module, name)?,

        SourceExpr::VarQual {
            kind: VarType::CapVar,
            module,
            name,
        } => {
            let ctor = env.find_ctor_qual(bump, region, module, name)?;
            to_var_ctor(bump, name, &ctor)
        }

        SourceExpr::List(exprs) => {
            CanExpr::List(canonicalize_exprs(bump, env, exprs, free_locals, warnings)?)
        }

        SourceExpr::Op(symbol) => {
            let binop = env.find_binop(bump, region, symbol)?;
            CanExpr::VarOperator {
                symbol,
                reference: QualifiedName {
                    home: binop.home,
                    name: binop.function,
                },
                annotation: None,
            }
        }

        SourceExpr::Negate(inner) => {
            CanExpr::Negate(canonicalize_expr(bump, env, inner, free_locals, warnings)?)
        }

        SourceExpr::BinOps { operands, last } => {
            return canonicalize_binops(bump, env, operands, last, region, free_locals, warnings);
        }

        SourceExpr::Lambda { parameters, body } => {
            return canonicalize_lambda(bump, env, parameters, body, region, free_locals, warnings);
        }

        SourceExpr::Call {
            function,
            arguments,
        } => {
            let can_func = canonicalize_expr(bump, env, function, free_locals, warnings)?;
            let can_args = canonicalize_exprs(bump, env, arguments, free_locals, warnings)?;
            CanExpr::Call {
                function: can_func,
                arguments: can_args,
            }
        }

        SourceExpr::If {
            branches,
            final_else,
        } => canonicalize_if(bump, env, branches, final_else, free_locals, warnings)?,

        SourceExpr::Let { defs, body } => {
            return canonicalize_let(bump, env, defs, body, region, free_locals, warnings);
        }

        SourceExpr::Case { scrutinee, arms } => {
            let can_scrutinee = canonicalize_expr(bump, env, scrutinee, free_locals, warnings)?;
            let can_branches = canonicalize_case_branches(bump, env, arms, free_locals, warnings)?;
            CanExpr::Case {
                scrutinee: can_scrutinee,
                branches: can_branches,
            }
        }

        SourceExpr::Accessor(field) => CanExpr::Accessor(field),

        SourceExpr::Access { record, field } => CanExpr::Access {
            record: canonicalize_expr(bump, env, record, free_locals, warnings)?,
            field,
        },

        SourceExpr::Update { record, fields } => {
            canonicalize_update(bump, env, record, fields, free_locals, warnings)?
        }

        SourceExpr::Record(fields) => {
            canonicalize_record(bump, env, fields, free_locals, warnings)?
        }

        SourceExpr::Unit => CanExpr::Unit,

        SourceExpr::Tuple {
            first,
            second,
            rest,
        } => {
            if rest.len() > 1 {
                return Err(vec![Error::TupleLargerThanThree { region }]);
            }
            CanExpr::Tuple {
                first: canonicalize_expr(bump, env, first, free_locals, warnings)?,
                second: canonicalize_expr(bump, env, second, free_locals, warnings)?,
                rest: canonicalize_exprs(bump, env, rest, free_locals, warnings)?,
            }
        }
    };
    Ok(bump.alloc(Located::at(region, can_expr)))
}

fn find_var<'a>(
    bump: &'a Bump,
    env: &Env<'a>,
    region: Region,
    name: &'a str,
    free_locals: &mut FreeLocals<'a>,
) -> Result<CanExpr<'a>, Vec<Error<'a>>> {
    match env.vars.get(name) {
        Some(Var::Local(_)) => {
            log_var(free_locals, name);
            Ok(CanExpr::VarLocal(name))
        }
        Some(Var::TopLevel(_)) => {
            log_var(free_locals, name);
            Ok(CanExpr::VarTopLevel(QualifiedName {
                home: env.home,
                name,
            }))
        }
        Some(Var::Foreign(home)) => Ok(CanExpr::VarTopLevel(QualifiedName { home: *home, name })),
        Some(Var::Foreigns(first, others)) => Err(vec![Error::AmbiguousVar {
            region,
            prefix: None,
            name,
            first_module: *first,
            other_modules: bump.alloc_slice_fill_iter(others.iter().copied()),
        }]),
        None => Err(vec![Error::NotFoundVar {
            region,
            prefix: None,
            name,
            suggestions: env.possible_var_names(bump),
        }]),
    }
}

fn find_var_qual<'a>(
    bump: &'a Bump,
    env: &Env<'a>,
    region: Region,
    prefix: &'a str,
    name: &'a str,
) -> Result<CanExpr<'a>, Vec<Error<'a>>> {
    let info = env
        .q_vars
        .get(prefix)
        .and_then(|m| m.get(name))
        .ok_or_else(|| {
            vec![Error::NotFoundVar {
                region,
                prefix: Some(prefix),
                name,
                suggestions: env.possible_var_names(bump),
            }]
        })?;
    match info {
        Info::Specific(home, ()) => Ok(CanExpr::VarTopLevel(QualifiedName { home: *home, name })),
        Info::Ambiguous(first, others) => Err(vec![Error::AmbiguousVar {
            region,
            prefix: Some(prefix),
            name,
            first_module: *first,
            other_modules: bump.alloc_slice_fill_iter(others.iter().copied()),
        }]),
    }
}

fn to_var_ctor<'a>(bump: &'a Bump, name: &'a str, ctor: &EnvCtor<'a>) -> CanExpr<'a> {
    match ctor {
        EnvCtor::Union {
            home,
            type_name,
            type_vars,
            index,
            arguments,
            options,
            ..
        } => {
            // Build: a -> b -> ... -> TypeName a b
            let free_vars: FreeVars<'a> = bump.alloc_slice_fill_iter(type_vars.iter().copied());
            let var_types: Vec<_> = type_vars
                .iter()
                .map(|v| &*bump.alloc(Located::at(Region::zero(), CanType::Var(v))))
                .collect();
            let result_type: &Located<CanType> = bump.alloc(Located::at(
                Region::zero(),
                CanType::Named {
                    reference: QualifiedName {
                        home: *home,
                        name: type_name,
                    },
                    args: bump.alloc_slice_fill_iter(var_types),
                },
            ));
            // foldr TLambda result args
            let mut typ: &Located<CanType> = result_type;
            for arg in arguments.iter().rev() {
                typ = bump.alloc(Located::at(
                    Region::zero(),
                    CanType::Lambda { from: arg, to: typ },
                ));
            }
            let annotation = bump.alloc(Annotation { free_vars, typ });

            CanExpr::VarConstructor {
                options: *options,
                reference: ConstructorName {
                    home: *home,
                    union: type_name,
                    name,
                },
                index: *index,
                annotation,
            }
        }
        EnvCtor::RecordCtor {
            home,
            field_names,
            field_types,
        } => {
            let free_vars_set: std::collections::BTreeSet<&str> = field_types
                .iter()
                .flat_map(|t| {
                    let mut vars = std::collections::BTreeSet::new();
                    types::collect_free_vars(&t.value, &mut vars);
                    vars
                })
                .collect();
            let free_vars: FreeVars<'a> = bump.alloc_slice_fill_iter(free_vars_set);

            let fields: Vec<nash_ast::FieldType<'a>> = field_names
                .iter()
                .zip(field_types.iter())
                .enumerate()
                .map(|(i, (fname, ftyp))| nash_ast::FieldType {
                    index: i as u16,
                    field: fname,
                    typ: ftyp,
                })
                .collect();
            let record_type: &Located<CanType> = bump.alloc(Located::at(
                Region::zero(),
                CanType::Record {
                    fields: bump.alloc_slice_fill_iter(fields),
                    ext: None,
                },
            ));
            let mut typ: &Located<CanType> = record_type;
            for field_type in field_types.iter().rev() {
                typ = bump.alloc(Located::at(
                    Region::zero(),
                    CanType::Lambda {
                        from: field_type,
                        to: typ,
                    },
                ));
            }
            let annotation = bump.alloc(Annotation { free_vars, typ });

            CanExpr::VarConstructor {
                options: CtorOpts::Normal,
                reference: ConstructorName {
                    home: *home,
                    union: name,
                    name,
                },
                index: 0,
                annotation,
            }
        }
    }
}

fn canonicalize_exprs<'a>(
    bump: &'a Bump,
    env: &Env<'a>,
    exprs: &[&'a Located<SourceExpr<'a>>],
    free_locals: &mut FreeLocals<'a>,
    warnings: &mut Vec<Warning<'a>>,
) -> Result<&'a [&'a Located<CanExpr<'a>>], Vec<Error<'a>>> {
    let mut results = Vec::with_capacity(exprs.len());
    let mut errors = Vec::new();
    for expr in exprs {
        match canonicalize_expr(bump, env, expr, free_locals, warnings) {
            Ok(can) => results.push(can),
            Err(errs) => errors.extend(errs),
        }
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    Ok(bump.alloc_slice_fill_iter(results))
}

fn canonicalize_lambda<'a>(
    bump: &'a Bump,
    env: &Env<'a>,
    parameters: &'a [&'a Located<nash_source::Pattern<'a>>],
    body: &'a Located<SourceExpr<'a>>,
    region: Region,
    free_locals: &mut FreeLocals<'a>,
    warnings: &mut Vec<Warning<'a>>,
) -> Result<&'a Located<CanExpr<'a>>, Vec<Error<'a>>> {
    let mut all_bindings = Bindings::new();
    let mut can_params = Vec::with_capacity(parameters.len());
    let mut errors = Vec::new();
    for param in parameters {
        match pattern::verify(bump, env, DuplicatePatternContext::LambdaArgs, param) {
            Ok((p, b)) => {
                can_params.push(p);
                all_bindings.extend(b);
            }
            Err(errs) => errors.extend(errs),
        }
    }
    if !errors.is_empty() {
        return Err(errors);
    }

    let inner_env = env.add_locals(&all_bindings)?;
    let mut body_free_locals = FreeLocals::new();
    let can_body = canonicalize_expr(bump, &inner_env, body, &mut body_free_locals, warnings)?;

    let outer_free = verify_bindings(
        WarningContext::Pattern,
        &all_bindings,
        body_free_locals,
        warnings,
    );
    merge_free_locals(free_locals, outer_free, true);

    Ok(bump.alloc(Located::at(
        region,
        CanExpr::Lambda {
            parameters: bump.alloc_slice_fill_iter(can_params),
            body: can_body,
        },
    )))
}

fn canonicalize_case_branches<'a>(
    bump: &'a Bump,
    env: &Env<'a>,
    arms: &[&'a CaseArm<'a>],
    free_locals: &mut FreeLocals<'a>,
    warnings: &mut Vec<Warning<'a>>,
) -> Result<&'a [CanCaseBranch<'a>], Vec<Error<'a>>> {
    let mut results = Vec::with_capacity(arms.len());
    let mut errors = Vec::new();
    for arm in arms {
        match canonicalize_case_branch(bump, env, arm, free_locals, warnings) {
            Ok(branch) => results.push(branch),
            Err(errs) => errors.extend(errs),
        }
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    Ok(bump.alloc_slice_fill_iter(results))
}

fn canonicalize_case_branch<'a>(
    bump: &'a Bump,
    env: &Env<'a>,
    arm: &'a CaseArm<'a>,
    free_locals: &mut FreeLocals<'a>,
    warnings: &mut Vec<Warning<'a>>,
) -> Result<CanCaseBranch<'a>, Vec<Error<'a>>> {
    let (can_pattern, bindings) =
        pattern::verify(bump, env, DuplicatePatternContext::CaseBranch, arm.pattern)?;
    let inner_env = env.add_locals(&bindings)?;
    let mut body_free_locals = FreeLocals::new();
    let can_body = canonicalize_expr(bump, &inner_env, arm.body, &mut body_free_locals, warnings)?;
    let outer_free = verify_bindings(
        WarningContext::Pattern,
        &bindings,
        body_free_locals,
        warnings,
    );
    merge_free_locals(free_locals, outer_free, false);
    Ok(CanCaseBranch {
        pattern: can_pattern,
        body: can_body,
    })
}

fn canonicalize_if<'a>(
    bump: &'a Bump,
    env: &Env<'a>,
    branches: &[&'a SourceIfBranch<'a>],
    final_else: &'a Located<SourceExpr<'a>>,
    free_locals: &mut FreeLocals<'a>,
    warnings: &mut Vec<Warning<'a>>,
) -> Result<CanExpr<'a>, Vec<Error<'a>>> {
    let mut can_branches = Vec::with_capacity(branches.len());
    let mut errors = Vec::new();
    for branch in branches {
        match (
            canonicalize_expr(bump, env, branch.condition, free_locals, warnings),
            canonicalize_expr(bump, env, branch.then_branch, free_locals, warnings),
        ) {
            (Ok(cond), Ok(then)) => can_branches.push(CanIfBranch {
                condition: cond,
                then_branch: then,
            }),
            (Err(e1), Err(e2)) => {
                errors.extend(e1);
                errors.extend(e2);
            }
            (Err(e), _) | (_, Err(e)) => errors.extend(e),
        }
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    let can_else = canonicalize_expr(bump, env, final_else, free_locals, warnings)?;
    let bs = bump.alloc_slice_fill_iter(can_branches);
    Ok(CanExpr::If {
        branches: bs,
        final_else: can_else,
    })
}

fn canonicalize_record<'a>(
    bump: &'a Bump,
    env: &Env<'a>,
    fields: &[&'a FieldAssign<'a>],
    free_locals: &mut FreeLocals<'a>,
    warnings: &mut Vec<Warning<'a>>,
) -> Result<CanExpr<'a>, Vec<Error<'a>>> {
    let field_iter = fields.iter().map(|f| (f.field.value, f.field.region));
    environment::dups::detect(field_iter, |name, first, second| Error::DuplicateField {
        name,
        first,
        second,
    })?;
    let mut can_fields = Vec::with_capacity(fields.len());
    let mut errors = Vec::new();
    for field in fields {
        match canonicalize_expr(bump, env, field.value, free_locals, warnings) {
            Ok(value) => can_fields.push(CanFieldValue {
                field: field.field,
                value,
            }),
            Err(errs) => errors.extend(errs),
        }
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    Ok(CanExpr::Record(bump.alloc_slice_fill_iter(can_fields)))
}

fn canonicalize_update<'a>(
    bump: &'a Bump,
    env: &Env<'a>,
    record: &'a Located<&'a str>,
    fields: &[&'a FieldAssign<'a>],
    free_locals: &mut FreeLocals<'a>,
    warnings: &mut Vec<Warning<'a>>,
) -> Result<CanExpr<'a>, Vec<Error<'a>>> {
    let base_expr = find_var(bump, env, record.region, record.value, free_locals)?;
    let base = bump.alloc(Located::at(record.region, base_expr));
    let field_iter = fields.iter().map(|f| (f.field.value, f.field.region));
    environment::dups::detect(field_iter, |name, first, second| Error::DuplicateField {
        name,
        first,
        second,
    })?;
    let mut can_fields = Vec::with_capacity(fields.len());
    let mut errors = Vec::new();
    for field in fields {
        match canonicalize_expr(bump, env, field.value, free_locals, warnings) {
            Ok(value) => {
                let f = field.field;
                can_fields.push(CanFieldUpdate {
                    field: f.value,
                    region: f.region,
                    value,
                });
            }
            Err(errs) => errors.extend(errs),
        }
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    Ok(CanExpr::Update {
        record: record.value,
        base,
        fields: bump.alloc_slice_fill_iter(can_fields),
    })
}

struct ResolvedOp<'a> {
    symbol: &'a str,
    home: ModuleName<'a>,
    function: &'a str,
    associativity: nash_ast::Associativity,
    precedence: nash_ast::Precedence,
}

fn canonicalize_binops<'a>(
    bump: &'a Bump,
    env: &Env<'a>,
    operands: &[&'a BinOpOperand<'a>],
    last: &'a Located<SourceExpr<'a>>,
    overall_region: Region,
    free_locals: &mut FreeLocals<'a>,
    warnings: &mut Vec<Warning<'a>>,
) -> Result<&'a Located<CanExpr<'a>>, Vec<Error<'a>>> {
    let mut can_exprs = Vec::with_capacity(operands.len() + 1);
    let mut ops = Vec::with_capacity(operands.len());
    let mut errors = Vec::new();
    for operand in operands {
        match canonicalize_expr(bump, env, operand.expr, free_locals, warnings) {
            Ok(e) => can_exprs.push(e),
            Err(errs) => errors.extend(errs),
        }
        match env.find_binop(bump, operand.op.region, operand.op.value) {
            Ok(binop) => ops.push(ResolvedOp {
                symbol: binop.symbol,
                home: binop.home,
                function: binop.function,
                associativity: binop.associativity,
                precedence: binop.precedence,
            }),
            Err(errs) => errors.extend(errs),
        }
    }
    match canonicalize_expr(bump, env, last, free_locals, warnings) {
        Ok(e) => can_exprs.push(e),
        Err(errs) => errors.extend(errs),
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    build_binop_tree(bump, &can_exprs, &ops, overall_region)
}

/// Precedence-climbing: find the root operator (lowest precedence),
/// recurse left and right. For left-assoc ties, rightmost is root.
/// For right-assoc ties, leftmost is root. Same-prec different-assoc = error.
fn build_binop_tree<'a>(
    bump: &'a Bump,
    exprs: &[&'a Located<CanExpr<'a>>],
    ops: &[ResolvedOp<'a>],
    overall_region: Region,
) -> Result<&'a Located<CanExpr<'a>>, Vec<Error<'a>>> {
    if ops.is_empty() {
        return Ok(exprs[0]);
    }
    build_tree_rec(bump, exprs, ops, 0, exprs.len() - 1, overall_region)
}

fn build_tree_rec<'a>(
    bump: &'a Bump,
    exprs: &[&'a Located<CanExpr<'a>>],
    ops: &[ResolvedOp<'a>],
    start: usize,
    end: usize,
    overall_region: Region,
) -> Result<&'a Located<CanExpr<'a>>, Vec<Error<'a>>> {
    if start == end {
        return Ok(exprs[start]);
    }

    // Find root: operator with lowest precedence in [start..end)
    let mut root_idx = start;
    for i in (start + 1)..end {
        if ops[i].precedence < ops[root_idx].precedence {
            root_idx = i;
        } else if ops[i].precedence == ops[root_idx].precedence {
            use nash_ast::Associativity::*;
            if ops[i].associativity != ops[root_idx].associativity {
                return Err(vec![Error::BinopConflict {
                    region: overall_region,
                    op1: ops[root_idx].symbol,
                    op2: ops[i].symbol,
                }]);
            }
            match ops[i].associativity {
                Left => root_idx = i,
                Right => {}
                None => {
                    return Err(vec![Error::BinopConflict {
                        region: overall_region,
                        op1: ops[root_idx].symbol,
                        op2: ops[i].symbol,
                    }]);
                }
            }
        }
    }

    let op = &ops[root_idx];
    let left = build_tree_rec(bump, exprs, ops, start, root_idx, overall_region)?;
    let right = build_tree_rec(bump, exprs, ops, root_idx + 1, end, overall_region)?;
    Ok(bump.alloc(Located::at(
        Region::span_across(&left.region, &right.region),
        CanExpr::Binop {
            symbol: op.symbol,
            reference: QualifiedName {
                home: op.home,
                name: op.function,
            },
            annotation: None,
            left,
            right,
        },
    )))
}

enum LetBinding<'a> {
    Define(&'a CanDef<'a>),
    Destruct(&'a Located<nash_ast::Pattern<'a>>, &'a Located<CanExpr<'a>>),
    Edge(&'a Located<&'a str>),
}

fn canonicalize_let<'a>(
    bump: &'a Bump,
    env: &Env<'a>,
    defs: &[&'a Located<SourceDef<'a>>],
    body: &'a Located<SourceExpr<'a>>,
    region: Region,
    free_locals: &mut FreeLocals<'a>,
    warnings: &mut Vec<Warning<'a>>,
) -> Result<&'a Located<CanExpr<'a>>, Vec<Error<'a>>> {
    let mut name_regions: Vec<(&'a str, Region)> = Vec::new();
    for def in defs {
        collect_def_names(&def.value, &mut name_regions);
    }
    let bindings = environment::dups::detect(name_regions.into_iter(), |name, first, second| {
        Error::DuplicatePattern {
            context: DuplicatePatternContext::LetBinding,
            name,
            first,
            second,
        }
    })?;

    let inner_env = env.add_locals(&bindings)?;

    let mut nodes: Vec<(&'a str, LetBinding<'a>)> = Vec::new();
    let mut edges: BTreeMap<&'a str, Vec<&'a str>> = BTreeMap::new();
    let mut def_free_locals_list: Vec<(bool, FreeLocals<'a>)> = Vec::new();
    let mut errors = Vec::new();
    for def in defs {
        match canonicalize_let_def(bump, &inner_env, &def.value, &bindings, warnings) {
            Ok((def_nodes, has_args, def_free)) => {
                for (name, binding, deps) in def_nodes {
                    edges.insert(name, deps);
                    nodes.push((name, binding));
                }
                def_free_locals_list.push((has_args, def_free));
            }
            Err(errs) => errors.extend(errs),
        }
    }
    if !errors.is_empty() {
        return Err(errors);
    }

    let mut combined_free_locals = FreeLocals::new();
    let can_body = canonicalize_expr(bump, &inner_env, body, &mut combined_free_locals, warnings)?;

    for (has_args, def_free) in def_free_locals_list {
        merge_free_locals(&mut combined_free_locals, def_free, has_args);
    }

    let outer_free = verify_bindings(
        WarningContext::Def,
        &bindings,
        combined_free_locals,
        warnings,
    );
    merge_free_locals(free_locals, outer_free, false);

    let node_names: Vec<&'a str> = nodes.iter().map(|(n, _)| *n).collect();
    let binding_map: BTreeMap<&'a str, LetBinding<'a>> = nodes.into_iter().collect();
    let sccs = scc::strongly_connected_components(&node_names, &edges);

    let mut result = can_body;
    for scc_group in sccs.into_iter().rev() {
        match scc_group {
            scc::Scc::Acyclic(name) => match binding_map.get(name) {
                Some(LetBinding::Define(def)) => {
                    result = bump.alloc(Located::at(
                        region,
                        CanExpr::Let {
                            definition: def,
                            body: result,
                        },
                    ));
                }
                Some(LetBinding::Destruct(pat, val)) => {
                    result = bump.alloc(Located::at(
                        region,
                        CanExpr::LetDestruct {
                            pattern: pat,
                            value: val,
                            body: result,
                        },
                    ));
                }
                Some(LetBinding::Edge(_)) | None => {}
            },
            scc::Scc::Cyclic(names) => {
                let cycle_defs = check_let_cycle(bump, &binding_map, &names)?;
                result = bump.alloc(Located::at(
                    region,
                    CanExpr::LetRec {
                        definitions: bump.alloc_slice_fill_iter(cycle_defs),
                        body: result,
                    },
                ));
            }
        }
    }
    Ok(result)
}

fn collect_def_names<'a>(def: &SourceDef<'a>, out: &mut Vec<(&'a str, Region)>) {
    match def {
        SourceDef::Define { name, .. } => out.push((name.value, name.region)),
        SourceDef::Destruct { pattern, .. } => {
            collect_pattern_names(&pattern.value, pattern.region, out);
        }
    }
}

fn collect_pattern_names<'a>(
    pat: &nash_source::Pattern<'a>,
    region: Region,
    out: &mut Vec<(&'a str, Region)>,
) {
    match pat {
        nash_source::Pattern::Var(name) => out.push((name, region)),
        nash_source::Pattern::Record(fields) => {
            for f in *fields {
                out.push((f.value, f.region));
            }
        }
        nash_source::Pattern::Alias { pattern, name } => {
            collect_pattern_names(&pattern.value, pattern.region, out);
            out.push((name.value, name.region));
        }
        nash_source::Pattern::Tuple {
            first,
            second,
            rest,
        } => {
            collect_pattern_names(&first.value, first.region, out);
            collect_pattern_names(&second.value, second.region, out);
            for r in *rest {
                collect_pattern_names(&r.value, r.region, out);
            }
        }
        nash_source::Pattern::Cons { head, tail } => {
            collect_pattern_names(&head.value, head.region, out);
            collect_pattern_names(&tail.value, tail.region, out);
        }
        _ => {}
    }
}

type LetDefResult<'a> = Result<
    (
        Vec<(&'a str, LetBinding<'a>, Vec<&'a str>)>,
        bool,
        FreeLocals<'a>,
    ),
    Vec<Error<'a>>,
>;

fn canonicalize_let_def<'a>(
    bump: &'a Bump,
    env: &Env<'a>,
    def: &SourceDef<'a>,
    let_bindings: &Bindings<'a>,
    warnings: &mut Vec<Warning<'a>>,
) -> LetDefResult<'a> {
    match def {
        SourceDef::Define {
            name,
            args,
            body,
            annotation,
        } => {
            let mut arg_bindings = Bindings::new();
            let mut can_args = Vec::with_capacity(args.len());
            for arg in *args {
                let (can_pat, bindings) = pattern::verify(
                    bump,
                    env,
                    DuplicatePatternContext::FuncArgs(name.value),
                    arg,
                )?;
                can_args.push(can_pat);
                arg_bindings.extend(bindings);
            }
            let body_env = env.add_locals(&arg_bindings)?;
            let mut body_free_locals = FreeLocals::new();
            let can_body =
                canonicalize_expr(bump, &body_env, body, &mut body_free_locals, warnings)?;

            let def_free = verify_bindings(
                WarningContext::Pattern,
                &arg_bindings,
                body_free_locals,
                warnings,
            );

            let deps: Vec<&'a str> = def_free
                .keys()
                .filter(|k| let_bindings.contains_key(*k))
                .copied()
                .collect();

            let has_args = !args.is_empty();
            let can_def: &'a CanDef<'a> = if let Some(ann) = annotation {
                let annotation_val = types::to_annotation(bump, env, ann)?;
                let typed_args = gather_typed_args(bump, name.value, &can_args, annotation_val)?;
                let result_type = peel_result_type(annotation_val, can_args.len());
                bump.alloc(CanDef::TypedDef {
                    name,
                    free_vars: annotation_val.free_vars,
                    args: typed_args,
                    body: can_body,
                    typ: result_type,
                })
            } else {
                bump.alloc(CanDef::Def {
                    name,
                    args: bump.alloc_slice_fill_iter(can_args),
                    body: can_body,
                })
            };

            Ok((
                vec![(name.value, LetBinding::Define(can_def), deps)],
                has_args,
                def_free,
            ))
        }
        SourceDef::Destruct { pattern, body } => {
            let (can_pattern, _) =
                pattern::verify(bump, env, DuplicatePatternContext::Destruct, pattern)?;
            let mut body_free_locals = FreeLocals::new();
            let can_body = canonicalize_expr(bump, env, body, &mut body_free_locals, warnings)?;
            let deps: Vec<&'a str> = body_free_locals
                .keys()
                .filter(|k| let_bindings.contains_key(*k))
                .copied()
                .collect();

            let mut names = Vec::new();
            collect_pattern_names(&pattern.value, pattern.region, &mut names);
            let synthetic = names.first().map(|(n, _)| *n).unwrap_or("_destruct");

            let mut result = vec![(synthetic, LetBinding::Destruct(can_pattern, can_body), deps)];
            for (pname, _) in &names {
                if *pname != synthetic {
                    result.push((
                        *pname,
                        LetBinding::Edge(bump.alloc(Located::at(pattern.region, *pname))),
                        vec![synthetic],
                    ));
                }
            }
            Ok((result, false, body_free_locals))
        }
    }
}

fn check_let_cycle<'a>(
    bump: &'a Bump,
    binding_map: &BTreeMap<&'a str, LetBinding<'a>>,
    names: &[&'a str],
) -> Result<Vec<&'a CanDef<'a>>, Vec<Error<'a>>> {
    let mut defs = Vec::new();
    for &name in names {
        match binding_map.get(name) {
            Some(LetBinding::Define(def)) => {
                let has_args = match def {
                    CanDef::Def { args, .. } => !args.is_empty(),
                    CanDef::TypedDef { args, .. } => !args.is_empty(),
                };
                if !has_args {
                    let def_name = match def {
                        CanDef::Def { name, .. } | CanDef::TypedDef { name, .. } => *name,
                    };
                    let others: Vec<&'a str> =
                        names.iter().filter(|n| **n != name).copied().collect();
                    return Err(vec![Error::RecursiveLet {
                        name: def_name,
                        others: bump.alloc_slice_fill_iter(others),
                    }]);
                }
                defs.push(*def);
            }
            Some(LetBinding::Edge(name_loc)) => {
                let others: Vec<&'a str> = names.iter().filter(|n| **n != name).copied().collect();
                return Err(vec![Error::RecursiveLet {
                    name: name_loc,
                    others: bump.alloc_slice_fill_iter(others),
                }]);
            }
            _ => {}
        }
    }
    Ok(defs)
}

pub fn gather_typed_args<'a>(
    bump: &'a Bump,
    func_name: &'a str,
    can_args: &[&'a Located<nash_ast::Pattern<'a>>],
    annotation: &'a Annotation<'a>,
) -> Result<&'a [CanTypedPattern<'a>], Vec<Error<'a>>> {
    let mut result = Vec::with_capacity(can_args.len());
    let mut current_type = annotation.typ;
    for arg in can_args {
        let dealiased = types::iterated_dealias(current_type);
        match &dealiased.value {
            CanType::Lambda { from, to } => {
                result.push(CanTypedPattern {
                    pattern: arg,
                    typ: from,
                });
                current_type = to;
            }
            _ => {
                return Err(vec![Error::AnnotationTooShort {
                    region: Region::span_across(
                        &can_args.first().unwrap().region,
                        &can_args.last().unwrap().region,
                    ),
                    name: func_name,
                }]);
            }
        }
    }
    Ok(bump.alloc_slice_fill_iter(result))
}

pub fn peel_result_type<'a>(
    annotation: &'a Annotation<'a>,
    arg_count: usize,
) -> &'a Located<nash_ast::Type<'a>> {
    let mut current = annotation.typ;
    for _ in 0..arg_count {
        let dealiased = types::iterated_dealias(current);
        if let CanType::Lambda { to, .. } = &dealiased.value {
            current = to;
        }
    }
    current
}
