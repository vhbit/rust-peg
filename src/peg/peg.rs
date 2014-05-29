use codegen::RustWriter;
use std::str;

pub struct Grammar {
	pub initializer: Option<String>,
	pub rules: Vec<Rule>,
}

pub struct Rule {
	pub name: String,
	pub expr: Box<Expr>,
	pub ret_type: String,
	pub exported: bool,
}

pub struct CharSetCase {
	pub start: char,
	pub end: char
}

pub struct TaggedExpr {
    pub name: Option<String>,
    pub expr: Box<Expr>
}

pub enum Expr {
	AnyCharExpr,
	LiteralExpr(String),
	CharSetExpr(bool, Vec<CharSetCase>),
	RuleExpr(String),
	SequenceExpr(Vec<Expr>),
	ChoiceExpr(Vec<Expr>),
	OptionalExpr(Box<Expr>),
	ZeroOrMore(Box<Expr>),
	OneOrMore(Box<Expr>),
	DelimitedExpr(Box<Expr>, Box<Expr>),
	PosAssertExpr(Box<Expr>),
	NegAssertExpr(Box<Expr>),
	StringifyExpr(Box<Expr>),
	ActionExpr(Vec<TaggedExpr>, String),
}

macro_rules! combine_str (
    ($e:expr, $($es:expr),+) => (
        ({
            let mut x = $e.to_string();

           $(x.push_str($es);)*
            x
        }).as_slice()
        )
        )


pub fn compile_grammar(w: &RustWriter, grammar: &Grammar) {
	compile_header(w, grammar.initializer.as_ref().map_or("", |s| s.as_slice()));

	for rule in grammar.rules.iter() {
		compile_rule(w, rule);
	}
}

fn compile_header(w: &RustWriter, header: &str) {
	w.write("// Generated by rust-peg. Do not edit.
use std::str::{CharRange};
    ");

 	w.write(header);

 	w.write("
#[inline]
fn slice_eq(input: &str, pos: uint, m: &str) -> Result<(uint, ()), uint> {
    let l = m.len();
    if input.len() >= pos + l && input.slice(pos, pos+l) == m {
        Ok((pos+l, ()))
    } else {
        Err(pos)
    }
}

#[inline]
fn any_char(input: &str, pos: uint) -> Result<(uint, ()), uint> {
    if input.len() > pos {
        Ok((input.char_range_at(pos).next, ()))
    } else {
        Err(pos)
    }
}

fn pos_to_line(input: &str, pos: uint) -> uint {
	let mut remaining = pos as int;
	let mut lineno: uint = 1;
	for line in input.lines() {
		remaining -= (line.len() as int) + 1;
		if remaining <= 0 {
			return lineno;
		}
		lineno+=1;
	}
	return lineno;
}
");
}


fn compile_rule(w: &RustWriter, rule: &Rule) {
    w.line("#[allow(unused_variable)]");

    w.def_fn(false, combine_str!("parse_", rule.name.as_slice()), "input: &str, pos: uint", combine_str!("Result<(uint, ", rule.ret_type.as_slice(),  ") , uint>"), || {
		compile_expr(w, rule.expr, rule.ret_type.as_slice() != "()");
	});

	if rule.exported {
		compile_rule_export(w, rule);
	}
}

fn compile_rule_export(w: &RustWriter, rule: &Rule) {
    w.def_fn(true, rule.name.as_slice(), "input: &str", combine_str!( "Result<", rule.ret_type.as_slice(), ", String>"), || {
	w.match_block(combine_str!("parse_", rule.name.as_slice(), "(input, 0)"), || {
	    w.match_case("Ok((pos, value))", || {
		w.if_else("pos == input.len()",
			  || { w.line("Ok(value)"); },
			  || { w.line("Err(format!(\"Expected end of input at {}\", pos_to_line(input, pos)))"); }
			  )
	    });
	    w.match_inline_case("Err(pos)", "Err(format!(\"Error at {}\", pos_to_line(input, pos)))");
	});
    });
}

fn compile_match_and_then(w: &RustWriter, e: &Expr, value_name: Option<&str>, then: ||) {
    w.let_block("seq_res", || {
	compile_expr(w, e, value_name.is_some());
    });
    w.match_block("seq_res", || {
	w.match_inline_case("Err(pos)", "Err(pos)");
	w.match_case(combine_str!("Ok((pos, ", value_name.unwrap_or("_"), "))"), || {
	    then();
	});
    });
}

fn compile_zero_or_more(w: &RustWriter, e: &Expr, list_initial: Option<&str>) {
	w.let_mut_stmt("repeat_pos", "pos");

	let result_used = match list_initial {
		Some(repeat_value) => {
			w.let_mut_stmt("repeat_value", repeat_value);
			true
		}
		_ => false
	};

	w.loop_block(|| {
		w.let_block("step_res", || {
			w.let_stmt("pos", "repeat_pos");
			compile_expr(w, e, result_used);
		});
		w.match_block("step_res", || {
			let match_arm = if result_used {
				"Ok((newpos, value))"
			} else {
				"Ok((newpos, _))"
			};
			w.match_case(match_arm, || {
				w.line("repeat_pos = newpos;");
				if result_used {
					w.line("repeat_value.push(value);");
				}
			});
			w.match_case("Err(..)", || {
				w.line("break;");
			});
		});
	});
	if result_used {
		w.line("Ok((repeat_pos, repeat_value))");
	} else {
		w.line("Ok((repeat_pos, ()))");
	}
}

fn compile_expr(w: &RustWriter, e: &Expr, result_used: bool) {
	match *e {
		AnyCharExpr => {
			w.line("any_char(input, pos)");
			/*w.if_else("input.len() > pos",
				||{ w.line("Ok(pos+1)"); },
				||{ w.line("Err(pos)"); }
			);*/
		}

		LiteralExpr(ref s) => {
			w.line(combine_str!("slice_eq(input, pos, \"", s.escape_default().as_slice(), "\")").as_slice());
			/*w.if_else("slice_eq(input, pos, \""+*s+"\")",
				||{ w.line("Ok(pos+" + s.len().to_str() + ")"); },
				||{ w.line("Err(pos)"); }
			);*/
		}

		CharSetExpr(invert, ref cases) => {
			w.if_else("input.len() > pos",
				|| {
					w.line("let CharRange {ch, next} = input.char_range_at(pos);");
					w.match_block("ch", || {
						w.write_indent();
						for (i, case) in cases.iter().enumerate() {
							if i != 0 { w.write(" | "); }
							if case.start == case.end {
								w.write(combine_str!("'", str::from_char(case.start).escape_default().as_slice(), "'"));
							} else {
								let start = str::from_char(case.start).escape_default();
								let end = str::from_char(case.end).escape_default();
								w.write(combine_str!("'", start.as_slice(), "'..'", end.as_slice(), "'"));
							}
						}
						w.write(combine_str!(" => { ", if !invert {"Ok((next, ()))"} else {"Err(pos)"}, " }\n"));
						w.match_inline_case("_", if !invert {"Err(pos)"} else {"Ok((next, ()))"});
					});
				},
				|| { w.line("Err(pos)"); }
			)
		}

		RuleExpr(ref ruleName) => {
			w.line(combine_str!("parse_", ruleName.as_slice(), "(input, pos)"));
		}

		SequenceExpr(ref exprs) => {
			fn write_seq(w: &RustWriter, exprs: &[Expr]) {
				if exprs.len() == 1 {
					compile_expr(w, &exprs[0], false);
				} else {
					compile_match_and_then(w, &exprs[0], None, || {
						write_seq(w, exprs.tail());
					});
				}
			}

			if exprs.len() > 0 {
				write_seq(w, exprs.as_slice());
			}
		}

		ChoiceExpr(ref exprs) => {
			fn write_choice(w: &RustWriter, exprs: &[Expr], result_used: bool) {
				if exprs.len() == 1 {
					compile_expr(w, &exprs[0], result_used);
				} else {
					w.let_block("choice_res", || {
						compile_expr(w, &exprs[0], result_used);
					});
					w.match_block("choice_res", || {
						w.match_inline_case("Ok((pos, value))", "Ok((pos, value))");
						w.match_case("Err(..)", || {
							write_choice(w, exprs.tail(), result_used);
						});
					});
				}
			}

			if exprs.len() > 0 {
				write_choice(w, exprs.as_slice(), result_used);
			}
		}

		OptionalExpr(ref e) => {
			w.let_block("optional_res", || {
				compile_expr(w, *e, result_used);
			});
			w.match_block("optional_res", || {
				w.match_inline_case("Ok((newpos, value))", "Ok((newpos, Some(value)))");
				w.match_inline_case("Err(..)", "Ok((pos, None))");
			});
		}

		ZeroOrMore(ref e) => {
			compile_zero_or_more(w, *e, if result_used { Some("vec!()") } else { None });
		}

		OneOrMore(ref e) => {
			compile_match_and_then(w, *e, if result_used { Some("first_value") } else { None }, || {
				compile_zero_or_more(w, *e, if result_used { Some("vec!(first_value)") } else { None });
			});
		}

		DelimitedExpr(_, _) => fail!("not implemented"),
		StringifyExpr(..) => fail!("not implemented"),

		PosAssertExpr(ref e) => {
			w.let_block("assert_res", || {
				compile_expr(w, *e, false);
			});
			w.match_block("assert_res", || {
				w.match_inline_case("Ok(..)", "Ok((pos, ()))");
				w.match_inline_case("Err(..)", "Err(pos)");
			});
		}

		NegAssertExpr(ref e) => {
			w.let_block("neg_assert_res", || {
				compile_expr(w, *e, false);
			});
			w.match_block("neg_assert_res", || {
				w.match_inline_case("Err(..)", "Ok((pos, ()))");
				w.match_inline_case("Ok(..)", "Err(pos)");
			});
		}

		ActionExpr(ref exprs, ref code) => {
			w.let_stmt("start_pos", "pos");
			fn write_seq(w: &RustWriter, exprs: &[TaggedExpr], code: &str) {
				match exprs.head() {
					Some(ref head) => {
						let name = head.name.as_ref().map(|s| s.as_slice());
						compile_match_and_then(w, head.expr, name, || {
							write_seq(w, exprs.tail(), code);
						});
					}
					None => {
						w.let_stmt("match_str",  "input.slice(start_pos, pos);");
						w.write_indent();
						w.write("Ok((pos, {");
						w.write(code);
						w.write("}))\n");
					}
				}
			}

			write_seq(w, exprs.as_slice(), code.as_slice());
		}
	}
}
