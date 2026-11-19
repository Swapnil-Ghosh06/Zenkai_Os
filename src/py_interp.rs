//! Zenkai OS - Real Ephemeral Python 3.11 Interpreter & .venv Runtime
//!
//! Author: Swapnil Ghosh 25BTRCL267 AIML-E
//! Genuine in-kernel Python evaluator supporting variables, expressions, lists,
//! dicts, tensors/arrays, functions, loops, conditionals, interactive REPL,
//! script execution, and strict ephemeral .venv module enforcement.

extern crate alloc;
use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use crate::pkg::SystemServices;
use crate::r19::{gen_bursty, simulate_lru, simulate_opt, REF_LENGTH};
use crate::timer::read_tsc;
use crate::{kerror, kinfo, kok, kprintln};

#[derive(Clone, Debug)]
pub enum PyVal {
    None,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    List(Vec<PyVal>),
    Dict(Vec<(String, PyVal)>),
    Tensor {
        data: Vec<f64>,
        kind: &'static str, // "tensor" or "array"
    },
    Module(String),
}

impl PyVal {
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::None => "NoneType",
            Self::Bool(_) => "bool",
            Self::Int(_) => "int",
            Self::Float(_) => "float",
            Self::Str(_) => "str",
            Self::List(_) => "list",
            Self::Dict(_) => "dict",
            Self::Tensor { kind, .. } => {
                if *kind == "tensor" {
                    "torch.Tensor"
                } else {
                    "numpy.ndarray"
                }
            }
            Self::Module(_) => "module",
        }
    }

    pub fn is_truthy(&self) -> bool {
        match self {
            Self::None => false,
            Self::Bool(b) => *b,
            Self::Int(i) => *i != 0,
            Self::Float(f) => *f != 0.0,
            Self::Str(s) => !s.is_empty(),
            Self::List(v) => !v.is_empty(),
            Self::Dict(d) => !d.is_empty(),
            Self::Tensor { data, .. } => !data.is_empty(),
            Self::Module(_) => true,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Self::Int(i) => Some(*i as f64),
            Self::Float(f) => Some(*f),
            Self::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
            _ => None,
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Self::Int(i) => Some(*i),
            Self::Float(f) => Some(*f as i64),
            Self::Bool(b) => Some(if *b { 1 } else { 0 }),
            _ => None,
        }
    }

    pub fn display_str(&self) -> String {
        match self {
            Self::None => String::from("None"),
            Self::Bool(b) => String::from(if *b { "True" } else { "False" }),
            Self::Int(i) => format!("{}", i),
            Self::Float(f) => format_f64(*f),
            Self::Str(s) => s.clone(),
            Self::List(items) => {
                let parts: Vec<String> = items.iter().map(|v| v.repr_str()).collect();
                format!("[{}]", parts.join(", "))
            }
            Self::Dict(entries) => {
                let parts: Vec<String> = entries
                    .iter()
                    .map(|(k, v)| format!("'{}': {}", k, v.repr_str()))
                    .collect();
                format!("{{{}}}", parts.join(", "))
            }
            Self::Tensor { data, kind } => {
                let preview: Vec<String> = data.iter().take(8).map(|x| format_f64(*x)).collect();
                let suffix = if data.len() > 8 { ", ..." } else { "" };
                format!("{}([{}{}])", kind, preview.join(", "), suffix)
            }
            Self::Module(name) => format!("<module '{}' (.venv)>", name),
        }
    }

    pub fn repr_str(&self) -> String {
        match self {
            Self::Str(s) => format!("'{}'", s),
            _ => self.display_str(),
        }
    }
}

fn format_f64(v: f64) -> String {
    if v.is_nan() {
        return String::from("nan");
    }
    if v.is_infinite() {
        return String::from(if v.is_sign_negative() { "-inf" } else { "inf" });
    }
    let neg = v < 0.0;
    let abs = if neg { -v } else { v };
    let whole = abs as i64;
    let frac = ((abs - whole as f64) * 10000.0 + 0.5) as i64;
    let (w, f) = if frac >= 10000 { (whole + 1, 0) } else { (whole, frac) };
    let prefix = if neg { "-" } else { "" };
    if f == 0 {
        format!("{}{}.0", prefix, w)
    } else {
        let mut s = format!("{}{}.{:04}", prefix, w, f);
        while s.ends_with('0') && !s.ends_with(".0") {
            s.pop();
        }
        s
    }
}

#[derive(Clone, Debug)]
pub struct PyFunc {
    pub params: Vec<String>,
    pub body: Vec<String>,
}

pub struct PyEnv {
    pub vars: BTreeMap<String, PyVal>,
    pub funcs: BTreeMap<String, PyFunc>,
    pub imported: BTreeMap<String, String>,
    pub repl_active: bool,
    pub block_buf: Vec<String>,
    pub rng_state: u64,
    call_depth: usize,
}

impl PyEnv {
    pub fn new() -> Self {
        Self {
            vars: BTreeMap::new(),
            funcs: BTreeMap::new(),
            imported: BTreeMap::new(),
            repl_active: false,
            block_buf: Vec::new(),
            rng_state: read_tsc() ^ 0x5DEECE66D,
            call_depth: 0,
        }
    }

    /// Purges all ephemeral .venv variables, functions, and imported modules.
    pub fn reset(&mut self) {
        self.vars.clear();
        self.funcs.clear();
        self.imported.clear();
        self.block_buf.clear();
    }

    fn next_rand(&mut self) -> f64 {
        self.rng_state = self.rng_state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        let bits = ((self.rng_state >> 33) as u32) as f64;
        bits / (u32::MAX as f64)
    }

    fn next_randn(&mut self) -> f64 {
        // Simple Box-Muller-like sum of uniforms (Irwin-Hall, mean 0, std ~1)
        let mut s = 0.0f64;
        for _ in 0..12 {
            s += self.next_rand();
        }
        s - 6.0
    }

    /// Checks whether a Python module is available in the current ephemeral .venv or system.
    pub fn is_module_available(mod_name: &str, services: &SystemServices) -> bool {
        let base = mod_name.split('.').next().unwrap_or(mod_name);
        let py_ready = services.is_apt_installed("python3-ml") || services.active_profile.is_some();
        if !py_ready {
            return false;
        }
        // Standard library always available when Python is installed
        if matches!(base, "math" | "random" | "sys" | "os" | "time" | "json") {
            return true;
        }
        // User-installed via `pip install <pkg>`
        if services.extra_pip_modules.iter().any(|m| {
            let clean = m.split("==").next().unwrap_or(m).trim();
            clean.eq_ignore_ascii_case(base)
                || (base == "sklearn" && clean.eq_ignore_ascii_case("scikit-learn"))
        }) {
            return true;
        }
        // Active ephemeral Role Profile wheels
        if let Some(prof) = services.active_profile_info() {
            for &m in prof.pip_modules {
                let clean = m.split("==").next().unwrap_or(m).trim();
                if clean.eq_ignore_ascii_case(base)
                    || (base == "sklearn" && clean.eq_ignore_ascii_case("scikit-learn"))
                    || (base == "zenkai" && prof.id == "ai-dev")
                {
                    return true;
                }
            }
        }
        // System-wide snap/apt packages
        match base {
            "torch" => services.is_snap_installed("pytorch-lite"),
            "numpy" => services.is_apt_installed("python3-ml"),
            "zenkai" => services.is_apt_installed("tensor-bench") || services.active_profile.is_some(),
            _ => false,
        }
    }

    /// Returns true if a shell line looks like Python code that can be evaluated inline.
    pub fn looks_like_python(line: &str) -> bool {
        let t = line.trim();
        if t.is_empty() {
            return false;
        }
        if t.starts_with("import ")
            || t.starts_with("from ")
            || t.starts_with("print(")
            || t.starts_with("def ")
            || t.starts_with("for ")
            || t.starts_with("while ")
            || t.starts_with("if ")
            || t.starts_with("len(")
            || t.starts_with("sum(")
            || t.starts_with("type(")
            || t.starts_with("range(")
            || t.starts_with("sorted(")
            || t.starts_with("dir(")
        {
            return true;
        }
        // Assignment like `a = 5` or `hi = "a"` or `x += 1`
        if let Some(eq) = t.find('=') {
            let lhs = t[..eq].trim().trim_end_matches(|c| matches!(c, '+' | '-' | '*' | '/'));
            if !lhs.is_empty()
                && lhs.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                && !t[eq + 1..].starts_with('=')
            {
                return true;
            }
        }
        // Function call `foo(...)` or arithmetic expression `2 + 3` or `a`
        if t.contains('(') && t.ends_with(')') {
            return true;
        }
        if t.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false)
            && t.chars().any(|c| matches!(c, '+' | '-' | '*' | '/' | '%'))
        {
            return true;
        }
        false
    }

    pub fn print_repl_banner(&self, services: &SystemServices) {
        let venv = services.active_venv_label().unwrap_or("global");
        kprintln!();
        kinfo!("PYTHON", "Python 3.11.4 (zenkai-uefi, env: .venv:{}) — Interactive REPL", venv);
        kprintln!("  Type Python code directly (import, variables, loops, print, math, numpy, torch).");
        kprintln!("  Type 'exit()' or 'quit()' or press [Esc] to return to shell.");
    }

    pub fn print_repl_prompt(&self) {
        let mut con = crate::console::GLOBAL_CONSOLE.lock();
        con.fg = crate::gop::Color::VIOLET;
        let prompt = if self.block_buf.is_empty() { ">>> " } else { "... " };
        let _ = core::fmt::write(&mut *con, format_args!("{}", prompt));
        con.fg = crate::gop::Color::TEXT_PRIMARY;
    }

    /// Handles a single line inside the interactive REPL. Returns `false` if REPL exited.
    pub fn handle_repl_line(&mut self, line: &str, services: &SystemServices) -> bool {
        let trimmed = line.trim();
        if self.block_buf.is_empty() {
            if trimmed == "exit()" || trimmed == "quit()" || trimmed == "exit" || trimmed == "quit" {
                self.repl_active = false;
                return false;
            }
            if trimmed.ends_with(':') {
                self.block_buf.push(String::from(line));
                return true;
            }
            if !trimmed.is_empty() {
                if let Err(e) = self.exec_line_interactive(line, services) {
                    kerror!("PYTHON", "{}", e);
                }
            }
            return true;
        } else {
            if trimmed.is_empty() {
                let lines = core::mem::take(&mut self.block_buf);
                if let Err(e) = self.exec_script_lines(&lines, services) {
                    kerror!("PYTHON", "{}", e);
                }
                return true;
            }
            self.block_buf.push(String::from(line));
            return true;
        }
    }

    /// Evaluates a single interactive line (prints expression results like Python REPL).
    pub fn exec_line_interactive(&mut self, raw: &str, services: &SystemServices) -> Result<(), String> {
        let t = raw.trim();
        if t.is_empty() || t.starts_with('#') {
            return Ok(());
        }
        // Check if it's a statement vs pure expression
        if Self::is_statement(t) {
            let lines = vec![String::from(t)];
            self.exec_script_lines(&lines, services)?;
            return Ok(());
        }
        let val = self.eval_expr(t, services)?;
        if !matches!(val, PyVal::None) {
            kprintln!("{}", val.repr_str());
        }
        Ok(())
    }

    fn is_statement(t: &str) -> bool {
        if t.starts_with("import ")
            || t.starts_with("from ")
            || t.starts_with("def ")
            || t.starts_with("return")
            || t.starts_with("if ")
            || t.starts_with("elif ")
            || t.starts_with("else:")
            || t.starts_with("for ")
            || t.starts_with("while ")
            || t.starts_with("pass")
        {
            return true;
        }
        // Assignment (`=`, `+=`, `-=`, `*=`, `/=`) outside quotes/parens/brackets
        let mut depth = 0i32;
        let mut in_sq = false;
        let mut in_dq = false;
        let bytes = t.as_bytes();
        for i in 0..bytes.len() {
            let b = bytes[i];
            if b == b'\'' && !in_dq { in_sq = !in_sq; }
            else if b == b'"' && !in_sq { in_dq = !in_dq; }
            else if !in_sq && !in_dq {
                if matches!(b, b'(' | b'[' | b'{') { depth += 1; }
                else if matches!(b, b')' | b']' | b'}') { depth -= 1; }
                else if depth == 0 && b == b'=' {
                    let prev = if i > 0 { bytes[i - 1] } else { 0 };
                    let next = if i + 1 < bytes.len() { bytes[i + 1] } else { 0 };
                    if prev != b'=' && prev != b'!' && prev != b'<' && prev != b'>' && next != b'=' {
                        return true;
                    }
                }
            }
        }
        false
    }

    pub fn exec_script(&mut self, code: &str, services: &SystemServices) -> Result<(), String> {
        let lines: Vec<String> = code
            .split(|c| c == '\n' || c == ';')
            .map(|s| String::from(s))
            .collect();
        self.exec_script_lines(&lines, services)?;
        Ok(())
    }

    pub fn exec_script_lines(&mut self, lines: &[String], services: &SystemServices) -> Result<Option<PyVal>, String> {
        let mut i = 0;
        while i < lines.len() {
            let raw = &lines[i];
            let trimmed = strip_comment(raw).trim();
            if trimmed.is_empty() {
                i += 1;
                continue;
            }
            let indent = leading_spaces(raw);

            // `def name(args):`
            if let Some(rest) = trimmed.strip_prefix("def ") {
                let colon = rest.find(':').ok_or_else(|| String::from("SyntaxError: expected ':' in def"))?;
                let sig = rest[..colon].trim();
                let lp = sig.find('(').ok_or_else(|| String::from("SyntaxError: expected '(' in def"))?;
                let rp = sig.rfind(')').ok_or_else(|| String::from("SyntaxError: expected ')' in def"))?;
                let fname = String::from(sig[..lp].trim());
                let params: Vec<String> = sig[lp + 1..rp]
                    .split(',')
                    .map(|p| String::from(p.trim()))
                    .filter(|p| !p.is_empty())
                    .collect();
                let (body, next_i) = collect_block(lines, i + 1, indent);
                self.funcs.insert(fname, PyFunc { params, body });
                i = next_i;
                continue;
            }

            // `if cond:` / `elif` / `else`
            if let Some(cond_rest) = trimmed.strip_prefix("if ") {
                let cond_str = cond_rest.trim_end_matches(':').trim();
                let mut cond_met = self.eval_expr(cond_str, services)?.is_truthy();
                let (if_body, mut next_i) = collect_block(lines, i + 1, indent);
                if cond_met {
                    if let Some(ret) = self.exec_script_lines(&if_body, services)? {
                        return Ok(Some(ret));
                    }
                }
                while next_i < lines.len() {
                    let nt = strip_comment(&lines[next_i]).trim();
                    let nind = leading_spaces(&lines[next_i]);
                    if nind != indent {
                        break;
                    }
                    if let Some(elif_rest) = nt.strip_prefix("elif ") {
                        let ec = elif_rest.trim_end_matches(':').trim();
                        let (ebody, ni) = collect_block(lines, next_i + 1, indent);
                        if !cond_met && self.eval_expr(ec, services)?.is_truthy() {
                            cond_met = true;
                            if let Some(ret) = self.exec_script_lines(&ebody, services)? {
                                return Ok(Some(ret));
                            }
                        }
                        next_i = ni;
                    } else if nt == "else:" {
                        let (ebody, ni) = collect_block(lines, next_i + 1, indent);
                        if !cond_met {
                            if let Some(ret) = self.exec_script_lines(&ebody, services)? {
                                return Ok(Some(ret));
                            }
                        }
                        next_i = ni;
                        break;
                    } else {
                        break;
                    }
                }
                i = next_i;
                continue;
            }

            // `for var in iterable:`
            if let Some(rest) = trimmed.strip_prefix("for ") {
                let in_pos = rest.find(" in ").ok_or_else(|| String::from("SyntaxError: expected 'in' in for loop"))?;
                let var_name = String::from(rest[..in_pos].trim());
                let iter_str = rest[in_pos + 4..].trim().trim_end_matches(':').trim();
                let (body, next_i) = collect_block(lines, i + 1, indent);
                let iter_val = self.eval_expr(iter_str, services)?;
                let items = match iter_val {
                    PyVal::List(v) => v,
                    PyVal::Tensor { data, .. } => data.into_iter().map(PyVal::Float).collect(),
                    PyVal::Str(s) => s.chars().map(|c| PyVal::Str(format!("{}", c))).collect(),
                    _ => return Err(format!("TypeError: '{}' object is not iterable", iter_val.type_name())),
                };
                for item in items.into_iter().take(500) {
                    self.vars.insert(var_name.clone(), item);
                    if let Some(ret) = self.exec_script_lines(&body, services)? {
                        return Ok(Some(ret));
                    }
                }
                i = next_i;
                continue;
            }

            // `while cond:`
            if let Some(rest) = trimmed.strip_prefix("while ") {
                let cond_str = rest.trim_end_matches(':').trim();
                let (body, next_i) = collect_block(lines, i + 1, indent);
                let mut guard = 0usize;
                while self.eval_expr(cond_str, services)?.is_truthy() {
                    if guard >= 500 {
                        return Err(String::from("RuntimeError: loop iteration limit (500) reached"));
                    }
                    guard += 1;
                    if let Some(ret) = self.exec_script_lines(&body, services)? {
                        return Ok(Some(ret));
                    }
                }
                i = next_i;
                continue;
            }

            if trimmed == "pass" {
                i += 1;
                continue;
            }

            if let Some(rest) = trimmed.strip_prefix("return") {
                let r = rest.trim();
                let val = if r.is_empty() { PyVal::None } else { self.eval_expr(r, services)? };
                return Ok(Some(val));
            }

            // `import a, b as c`
            if let Some(rest) = trimmed.strip_prefix("import ") {
                for item in rest.split(',') {
                    let item = item.trim();
                    if let Some(as_pos) = item.find(" as ") {
                        let mname = item[..as_pos].trim();
                        let alias = item[as_pos + 4..].trim();
                        self.do_import(mname, alias, services)?;
                    } else {
                        self.do_import(item, item, services)?;
                    }
                }
                i += 1;
                continue;
            }

            // `from mod import x [as y]`
            if let Some(rest) = trimmed.strip_prefix("from ") {
                let imp_pos = rest.find(" import ").ok_or_else(|| String::from("SyntaxError: expected 'import'"))?;
                let mname = rest[..imp_pos].trim();
                if !Self::is_module_available(mname, services) {
                    return Err(format!("ModuleNotFoundError: No module named '{}'", mname));
                }
                let syms = rest[imp_pos + 8..].trim();
                for sym in syms.split(',') {
                    let sym = sym.trim();
                    let (orig, alias) = if let Some(ap) = sym.find(" as ") {
                        (sym[..ap].trim(), sym[ap + 4..].trim())
                    } else {
                        (sym, sym)
                    };
                    self.vars.insert(String::from(alias), PyVal::Module(format!("{}.{}", mname, orig)));
                }
                i += 1;
                continue;
            }

            // Augmented assignments (+=, -=, *=, /=)
            let mut handled_aug = false;
            for (op, fn_idx) in [(" += ", 0), (" -= ", 1), (" *= ", 2), (" /= ", 3), ("+=", 0), ("-=", 1), ("*=", 2), ("/=", 3)] {
                if let Some(pos) = find_top_level(trimmed, op) {
                    let lhs = trimmed[..pos].trim();
                    let rhs = trimmed[pos + op.len()..].trim();
                    if lhs.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                        let cur = self.vars.get(lhs).cloned().ok_or_else(|| format!("NameError: name '{}' is not defined", lhs))?;
                        let rval = self.eval_expr(rhs, services)?;
                        let bin = match fn_idx {
                            0 => "+",
                            1 => "-",
                            2 => "*",
                            _ => "/",
                        };
                        let res = self.apply_binop(cur, bin, rval)?;
                        self.vars.insert(String::from(lhs), res);
                        handled_aug = true;
                        break;
                    }
                }
            }
            if handled_aug {
                i += 1;
                continue;
            }

            // Standard assignment `lhs = rhs`
            if Self::is_statement(trimmed) {
                if let Some(eq) = find_top_level(trimmed, "=") {
                    let lhs = trimmed[..eq].trim();
                    let rhs = trimmed[eq + 1..].trim();
                    let val = self.eval_expr(rhs, services)?;
                    if lhs.contains(',') {
                        let names: Vec<&str> = lhs.split(',').map(|s| s.trim()).collect();
                        if let PyVal::List(items) = val {
                            for (n, v) in names.iter().zip(items.into_iter()) {
                                self.vars.insert(String::from(*n), v);
                            }
                        } else {
                            return Err(String::from("ValueError: cannot unpack non-sequence"));
                        }
                    } else {
                        self.vars.insert(String::from(lhs), val);
                    }
                    i += 1;
                    continue;
                }
            }

            // Expression statement
            let _ = self.eval_expr(trimmed, services)?;
            i += 1;
        }
        Ok(None)
    }

    fn do_import(&mut self, mod_name: &str, alias: &str, services: &SystemServices) -> Result<(), String> {
        if !Self::is_module_available(mod_name, services) {
            return Err(format!(
                "ModuleNotFoundError: No module named '{}' (activate a .venv profile or run 'pip install {}')",
                mod_name, mod_name
            ));
        }
        self.imported.insert(String::from(alias), String::from(mod_name));
        self.vars.insert(String::from(alias), PyVal::Module(String::from(mod_name)));
        Ok(())
    }

    pub fn eval_expr(&mut self, expr: &str, services: &SystemServices) -> Result<PyVal, String> {
        let t = expr.trim();
        if t.is_empty() {
            return Ok(PyVal::None);
        }
        // Strip outer matching parentheses
        if t.starts_with('(') && t.ends_with(')') && matching_paren_end(t, 0) == Some(t.len() - 1) {
            return self.eval_expr(&t[1..t.len() - 1], services);
        }

        // Boolean operators: `or`, `and`
        if let Some(pos) = find_top_level_kw(t, "or") {
            let l = self.eval_expr(&t[..pos], services)?;
            if l.is_truthy() {
                return Ok(l);
            }
            return self.eval_expr(&t[pos + 2..], services);
        }
        if let Some(pos) = find_top_level_kw(t, "and") {
            let l = self.eval_expr(&t[..pos], services)?;
            if !l.is_truthy() {
                return Ok(l);
            }
            return self.eval_expr(&t[pos + 3..], services);
        }
        if let Some(rest) = t.strip_prefix("not ") {
            let v = self.eval_expr(rest, services)?;
            return Ok(PyVal::Bool(!v.is_truthy()));
        }

        // Comparison operators
        for op in ["==", "!=", "<=", ">=", "<", ">"] {
            if let Some(pos) = find_top_level_cmp(t, op) {
                let l = self.eval_expr(&t[..pos], services)?;
                let r = self.eval_expr(&t[pos + op.len()..], services)?;
                return Ok(PyVal::Bool(compare_vals(&l, op, &r)?));
            }
        }

        // Addition / Subtraction (rightmost top-level)
        if let Some((pos, op)) = find_rightmost_addsub(t) {
            let l = self.eval_expr(&t[..pos], services)?;
            let r = self.eval_expr(&t[pos + 1..], services)?;
            return self.apply_binop(l, op, r);
        }

        // Multiplication / Division / Modulo
        if let Some((pos, op)) = find_rightmost_muldiv(t) {
            let l = self.eval_expr(&t[..pos], services)?;
            let r = self.eval_expr(&t[pos + op.len()..], services)?;
            return self.apply_binop(l, op, r);
        }

        // Unary +/-
        if let Some(rest) = t.strip_prefix('-') {
            let v = self.eval_expr(rest, services)?;
            return match v {
                PyVal::Int(i) => Ok(PyVal::Int(-i)),
                PyVal::Float(f) => Ok(PyVal::Float(-f)),
                _ => Err(format!("TypeError: bad operand type for unary -: '{}'", v.type_name())),
            };
        }

        // Power `**`
        if let Some(pos) = find_top_level(t, "**") {
            let l = self.eval_expr(&t[..pos], services)?;
            let r = self.eval_expr(&t[pos + 2..], services)?;
            return self.apply_binop(l, "**", r);
        }

        // Literals: None, True, False
        match t {
            "None" => return Ok(PyVal::None),
            "True" => return Ok(PyVal::Bool(true)),
            "False" => return Ok(PyVal::Bool(false)),
            _ => {}
        }

        // f-string `f"..."`
        if (t.starts_with("f\"") && t.ends_with('"')) || (t.starts_with("f'") && t.ends_with('\'')) {
            let inner = &t[2..t.len() - 1];
            return Ok(PyVal::Str(self.eval_fstring(inner, services)?));
        }

        // String literal
        if (t.starts_with('"') && t.ends_with('"') && t.len() >= 2)
            || (t.starts_with('\'') && t.ends_with('\'') && t.len() >= 2)
        {
            return Ok(PyVal::Str(String::from(&t[1..t.len() - 1])));
        }

        // List literal or list comprehension `[...]`
        if t.starts_with('[') && t.ends_with(']') {
            let inner = t[1..t.len() - 1].trim();
            if inner.is_empty() {
                return Ok(PyVal::List(Vec::new()));
            }
            if let Some(for_pos) = find_top_level_kw(inner, "for") {
                let expr_part = inner[..for_pos].trim();
                let rest = inner[for_pos + 3..].trim();
                if let Some(in_pos) = find_top_level_kw(rest, "in") {
                    let var = rest[..in_pos].trim();
                    let iter_expr = rest[in_pos + 2..].trim();
                    let iter_val = self.eval_expr(iter_expr, services)?;
                    let items = match iter_val {
                        PyVal::List(v) => v,
                        _ => return Err(String::from("TypeError: comprehension target is not iterable")),
                    };
                    let mut out = Vec::new();
                    for item in items.into_iter().take(200) {
                        self.vars.insert(String::from(var), item);
                        out.push(self.eval_expr(expr_part, services)?);
                    }
                    return Ok(PyVal::List(out));
                }
            }
            let elems = split_top_level(inner, ',');
            let mut out = Vec::new();
            for e in elems {
                if !e.trim().is_empty() {
                    out.push(self.eval_expr(e, services)?);
                }
            }
            return Ok(PyVal::List(out));
        }

        // Dict literal `{k: v}`
        if t.starts_with('{') && t.ends_with('}') {
            let inner = t[1..t.len() - 1].trim();
            if inner.is_empty() {
                return Ok(PyVal::Dict(Vec::new()));
            }
            let mut entries = Vec::new();
            for pair in split_top_level(inner, ',') {
                if let Some(colon) = find_top_level(pair, ":") {
                    let k = self.eval_expr(&pair[..colon], services)?.display_str();
                    let v = self.eval_expr(&pair[colon + 1..], services)?;
                    entries.push((k, v));
                }
            }
            return Ok(PyVal::Dict(entries));
        }

        // Numbers
        if let Ok(iv) = t.parse::<i64>() {
            return Ok(PyVal::Int(iv));
        }
        if let Ok(fv) = t.parse::<f64>() {
            return Ok(PyVal::Float(fv));
        }

        // Indexing `obj[idx]`
        if t.ends_with(']') {
            if let Some(lb) = find_matching_open_bracket(t, t.len() - 1) {
                let target = self.eval_expr(&t[..lb], services)?;
                let idx_val = self.eval_expr(&t[lb + 1..t.len() - 1], services)?;
                return eval_index(&target, &idx_val);
            }
        }

        // Function / Method call `callee(args)`
        if t.ends_with(')') {
            if let Some(lp) = find_matching_open_paren(t, t.len() - 1) {
                let callee = t[..lp].trim();
                let args_str = &t[lp + 1..t.len() - 1];
                let raw_args = split_top_level(args_str, ',');
                let mut pos_args = Vec::new();
                let mut kw_args: Vec<(String, PyVal)> = Vec::new();
                for a in raw_args {
                    let a = a.trim();
                    if a.is_empty() {
                        continue;
                    }
                    if let Some(eq) = find_top_level(a, "=") {
                        let k = String::from(a[..eq].trim());
                        let v = self.eval_expr(&a[eq + 1..], services)?;
                        kw_args.push((k, v));
                    } else {
                        pos_args.push(self.eval_expr(a, services)?);
                    }
                }
                return self.eval_call(callee, pos_args, kw_args, services);
            }
        }

        // Attribute access `obj.attr`
        if let Some(dot) = find_last_top_level_dot(t) {
            let obj_str = t[..dot].trim();
            let attr = t[dot + 1..].trim();
            let obj = self.eval_expr(obj_str, services)?;
            return self.eval_attr(&obj, attr, services);
        }

        // Variable lookup
        if let Some(val) = self.vars.get(t) {
            return Ok(val.clone());
        }

        Err(format!("NameError: name '{}' is not defined", t))
    }

    fn eval_fstring(&mut self, inner: &str, services: &SystemServices) -> Result<String, String> {
        let mut out = String::new();
        let bytes = inner.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'{' {
                if let Some(end) = inner[i + 1..].find('}') {
                    let expr_s = &inner[i + 1..i + 1 + end];
                    let val = self.eval_expr(expr_s, services)?;
                    out.push_str(&val.display_str());
                    i += end + 2;
                    continue;
                }
            }
            out.push(bytes[i] as char);
            i += 1;
        }
        Ok(out)
    }

    fn eval_attr(&self, obj: &PyVal, attr: &str, services: &SystemServices) -> Result<PyVal, String> {
        match (obj, attr) {
            (PyVal::Tensor { data, .. }, "shape") => Ok(PyVal::List(vec![PyVal::Int(data.len() as i64)])),
            (PyVal::Tensor { data, .. }, "size") => Ok(PyVal::Int(data.len() as i64)),
            (PyVal::Module(m), "version") | (PyVal::Module(m), "__version__") => {
                let ver = match m.as_str() {
                    "torch" => "2.1.0+zenkai",
                    "numpy" => "1.26.4",
                    "transformers" => "4.40.0",
                    "sklearn" | "scikit-learn" => "1.4.2",
                    "onnxruntime" => "1.17.1",
                    _ => "1.0.0",
                };
                Ok(PyVal::Str(String::from(ver)))
            }
            (PyVal::Module(m), "pi") if m == "math" => Ok(PyVal::Float(core::f64::consts::PI)),
            (PyVal::Module(m), "e") if m == "math" => Ok(PyVal::Float(core::f64::consts::E)),
            (PyVal::Module(m), "platform") if m == "sys" => Ok(PyVal::Str(String::from("zenkai-uefi"))),
            (PyVal::Module(m), "version") if m == "sys" => Ok(PyVal::Str(String::from("3.11.4 (Zenkai OS)"))),
            (PyVal::Module(m), "venv") if m == "sys" => Ok(PyVal::Str(String::from(services.active_venv_label().unwrap_or("none")))),
            _ => Err(format!("AttributeError: '{}' object has no attribute '{}'", obj.type_name(), attr)),
        }
    }

    fn eval_call(
        &mut self,
        callee: &str,
        args: Vec<PyVal>,
        kw: Vec<(String, PyVal)>,
        services: &SystemServices,
    ) -> Result<PyVal, String> {
        // Method call `obj.method(...)`
        if let Some(dot) = find_last_top_level_dot(callee) {
            let obj_name = callee[..dot].trim();
            let method = callee[dot + 1..].trim();

            // Check if `obj_name` is a module alias or `zenkai.r19`
            if let Some(PyVal::Module(mod_name)) = self.vars.get(obj_name).cloned() {
                return self.eval_module_call(&mod_name, method, args, kw, services);
            }
            if obj_name.starts_with("torch.") || obj_name.starts_with("np.") || obj_name.starts_with("numpy.") {
                return self.eval_module_call(obj_name, method, args, kw, services);
            }

            let obj = self.eval_expr(obj_name, services)?;
            let res = self.eval_method_call(obj, method, &args)?;
            // Mutate variable in-place for list.append / list.pop
            if matches!(method, "append" | "pop" | "clear") {
                if let PyVal::List(_) = res.0 {
                    self.vars.insert(String::from(obj_name), res.0);
                }
            }
            return Ok(res.1);
        }

        // User-defined function (guarded against bare-metal stack overflow)
        if let Some(func) = self.funcs.get(callee).cloned() {
            if self.call_depth >= 24 {
                return Err(String::from("RecursionError: maximum recursion depth (24) exceeded"));
            }
            self.call_depth += 1;
            let saved = self.vars.clone();
            for (p, v) in func.params.iter().zip(args.into_iter()) {
                self.vars.insert(p.clone(), v);
            }
            let ret = self.exec_script_lines(&func.body, services);
            for p in &func.params {
                if let Some(old) = saved.get(p) {
                    self.vars.insert(p.clone(), old.clone());
                } else {
                    self.vars.remove(p);
                }
            }
            self.call_depth -= 1;
            return Ok(ret?.unwrap_or(PyVal::None));
        }

        // Builtins
        match callee {
            "print" => {
                let parts: Vec<String> = args.iter().map(|a| a.display_str()).collect();
                kprintln!("{}", parts.join(" "));
                Ok(PyVal::None)
            }
            "len" => {
                let a = args.first().ok_or_else(|| String::from("TypeError: len() takes 1 argument"))?;
                match a {
                    PyVal::Str(s) => Ok(PyVal::Int(s.len() as i64)),
                    PyVal::List(v) => Ok(PyVal::Int(v.len() as i64)),
                    PyVal::Dict(d) => Ok(PyVal::Int(d.len() as i64)),
                    PyVal::Tensor { data, .. } => Ok(PyVal::Int(data.len() as i64)),
                    _ => Err(format!("TypeError: object of type '{}' has no len()", a.type_name())),
                }
            }
            "range" => {
                let (start, stop, step) = match args.len() {
                    1 => (0, args[0].as_i64().unwrap_or(0), 1),
                    2 => (args[0].as_i64().unwrap_or(0), args[1].as_i64().unwrap_or(0), 1),
                    3 => (
                        args[0].as_i64().unwrap_or(0),
                        args[1].as_i64().unwrap_or(0),
                        args[2].as_i64().unwrap_or(1).max(1),
                    ),
                    _ => return Err(String::from("TypeError: range() expects 1..3 arguments")),
                };
                let mut out = Vec::new();
                let mut cur = start;
                while cur < stop && out.len() < 500 {
                    out.push(PyVal::Int(cur));
                    cur += step;
                }
                Ok(PyVal::List(out))
            }
            "sum" => {
                let a = args.first().ok_or_else(|| String::from("TypeError: sum() takes 1 argument"))?;
                match a {
                    PyVal::List(v) => {
                        let mut is_f = false;
                        let mut acc = 0.0f64;
                        for x in v {
                            if matches!(x, PyVal::Float(_)) { is_f = true; }
                            acc += x.as_f64().ok_or_else(|| String::from("TypeError: unsupported operand in sum()"))?;
                        }
                        if is_f { Ok(PyVal::Float(acc)) } else { Ok(PyVal::Int(acc as i64)) }
                    }
                    PyVal::Tensor { data, .. } => Ok(PyVal::Float(data.iter().sum())),
                    _ => Err(String::from("TypeError: sum() argument is not iterable")),
                }
            }
            "min" | "max" => {
                let vals: Vec<f64> = if args.len() == 1 {
                    match &args[0] {
                        PyVal::List(v) => v.iter().filter_map(|x| x.as_f64()).collect(),
                        PyVal::Tensor { data, .. } => data.clone(),
                        _ => vec![],
                    }
                } else {
                    args.iter().filter_map(|x| x.as_f64()).collect()
                };
                if vals.is_empty() {
                    return Err(String::from("ValueError: arg is an empty sequence"));
                }
                let mut best = vals[0];
                for &x in &vals[1..] {
                    if (callee == "min" && x < best) || (callee == "max" && x > best) {
                        best = x;
                    }
                }
                if (best - (best as i64 as f64)).abs() < 1e-12 {
                    Ok(PyVal::Int(best as i64))
                } else {
                    Ok(PyVal::Float(best))
                }
            }
            "abs" => {
                let a = args.first().ok_or_else(|| String::from("TypeError: abs() takes 1 argument"))?;
                match a {
                    PyVal::Int(i) => Ok(PyVal::Int(i.abs())),
                    PyVal::Float(f) => Ok(PyVal::Float(f.abs())),
                    _ => Err(String::from("TypeError: bad operand type for abs()")),
                }
            }
            "round" => {
                let f = args.first().and_then(|x| x.as_f64()).ok_or_else(|| String::from("TypeError: round() expects number"))?;
                let digits = args.get(1).and_then(|x| x.as_i64()).unwrap_or(0) as i32;
                let mut scale = 1.0f64;
                for _ in 0..digits.max(0) { scale *= 10.0; }
                let rounded = ((f * scale) + if f >= 0.0 { 0.5 } else { -0.5 }) as i64 as f64 / scale;
                if digits == 0 { Ok(PyVal::Int(rounded as i64)) } else { Ok(PyVal::Float(rounded)) }
            }
            "type" => {
                let a = args.first().ok_or_else(|| String::from("TypeError: type() takes 1 argument"))?;
                Ok(PyVal::Str(format!("<class '{}'>", a.type_name())))
            }
            "int" => Ok(PyVal::Int(match args.first() {
                Some(PyVal::Str(s)) => s.trim().parse::<i64>().unwrap_or(0),
                Some(v) => v.as_i64().unwrap_or(0),
                None => 0,
            })),
            "float" => Ok(PyVal::Float(match args.first() {
                Some(PyVal::Str(s)) => s.trim().parse::<f64>().unwrap_or(0.0),
                Some(v) => v.as_f64().unwrap_or(0.0),
                None => 0.0,
            })),
            "str" => Ok(PyVal::Str(args.first().map(|v| v.display_str()).unwrap_or_default())),
            "bool" => Ok(PyVal::Bool(args.first().map(|v| v.is_truthy()).unwrap_or(false))),
            "list" => match args.first() {
                Some(PyVal::List(v)) => Ok(PyVal::List(v.clone())),
                Some(PyVal::Tensor { data, .. }) => Ok(PyVal::List(data.iter().map(|&x| PyVal::Float(x)).collect())),
                Some(PyVal::Str(s)) => Ok(PyVal::List(s.chars().map(|c| PyVal::Str(format!("{}", c))).collect())),
                None => Ok(PyVal::List(Vec::new())),
                _ => Err(String::from("TypeError: argument is not iterable")),
            },
            "sorted" => {
                if let Some(PyVal::List(v)) = args.first() {
                    let mut out = v.clone();
                    out.sort_by(|a, b| {
                        a.as_f64()
                            .unwrap_or(0.0)
                            .partial_cmp(&b.as_f64().unwrap_or(0.0))
                            .unwrap_or(core::cmp::Ordering::Equal)
                    });
                    Ok(PyVal::List(out))
                } else {
                    Err(String::from("TypeError: sorted() expects a list"))
                }
            }
            "dir" => {
                let mut names: Vec<PyVal> = self.vars.keys().map(|k| PyVal::Str(k.clone())).collect();
                for f in self.funcs.keys() {
                    names.push(PyVal::Str(f.clone()));
                }
                Ok(PyVal::List(names))
            }
            "PageSimulator" => {
                let frames = kw
                    .iter()
                    .find(|(k, _)| k == "frames")
                    .and_then(|(_, v)| v.as_i64())
                    .or_else(|| args.get(1).and_then(|v| v.as_i64()))
                    .unwrap_or(12)
                    .clamp(2, 64) as usize;
                let mut refs = vec![0u8; REF_LENGTH];
                gen_bursty(&mut refs, 0x5A17_0019);
                let lru_faults = simulate_lru(&refs, frames);
                let opt_faults = simulate_opt(&refs, frames);
                kok!(
                    "R19-SIM",
                    "PageSimulator(frames={}) -> LRU faults: {}, OPT faults: {}",
                    frames,
                    lru_faults,
                    opt_faults
                );
                Ok(PyVal::Float((opt_faults as f64) / (REF_LENGTH as f64)))
            }
            "exit" | "quit" => {
                self.repl_active = false;
                Ok(PyVal::None)
            }
            _ => Err(format!("NameError: name '{}' is not defined", callee)),
        }
    }

    fn eval_module_call(
        &mut self,
        mod_name: &str,
        method: &str,
        args: Vec<PyVal>,
        kw: Vec<(String, PyVal)>,
        services: &SystemServices,
    ) -> Result<PyVal, String> {
        let base = mod_name.split('.').next().unwrap_or(mod_name);
        match base {
            "numpy" => {
                let kind = "array";
                match method {
                    "array" => {
                        let data = extract_floats(args.first())?;
                        Ok(PyVal::Tensor { data, kind })
                    }
                    "zeros" => {
                        let n = args.first().and_then(|v| v.as_i64()).unwrap_or(4).clamp(1, 256) as usize;
                        Ok(PyVal::Tensor { data: vec![0.0; n], kind })
                    }
                    "ones" => {
                        let n = args.first().and_then(|v| v.as_i64()).unwrap_or(4).clamp(1, 256) as usize;
                        Ok(PyVal::Tensor { data: vec![1.0; n], kind })
                    }
                    "arange" => {
                        let start = if args.len() >= 2 { args[0].as_f64().unwrap_or(0.0) } else { 0.0 };
                        let stop = if args.len() >= 2 { args[1].as_f64().unwrap_or(5.0) } else { args.first().and_then(|v| v.as_f64()).unwrap_or(5.0) };
                        let step = args.get(2).and_then(|v| v.as_f64()).unwrap_or(1.0).max(0.01);
                        let mut data = Vec::new();
                        let mut x = start;
                        while x < stop && data.len() < 256 {
                            data.push(x);
                            x += step;
                        }
                        Ok(PyVal::Tensor { data, kind })
                    }
                    "dot" => {
                        let a = extract_floats(args.first())?;
                        let b = extract_floats(args.get(1))?;
                        let sum: f64 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
                        Ok(PyVal::Float(sum))
                    }
                    "mean" => {
                        let a = extract_floats(args.first())?;
                        if a.is_empty() { return Ok(PyVal::Float(0.0)); }
                        Ok(PyVal::Float(a.iter().sum::<f64>() / a.len() as f64))
                    }
                    "sum" => {
                        let a = extract_floats(args.first())?;
                        Ok(PyVal::Float(a.iter().sum()))
                    }
                    "randn" => {
                        let n = args.iter().filter_map(|v| v.as_i64()).fold(1i64, |acc, x| acc * x).clamp(1, 128) as usize;
                        let mut data = Vec::with_capacity(n);
                        for _ in 0..n { data.push(self.next_randn()); }
                        Ok(PyVal::Tensor { data, kind })
                    }
                    _ => Err(format!("AttributeError: module 'numpy' has no attribute '{}'", method)),
                }
            }
            "torch" => {
                let kind = "tensor";
                match method {
                    "tensor" => {
                        let data = extract_floats(args.first())?;
                        Ok(PyVal::Tensor { data, kind })
                    }
                    "zeros" => {
                        let n = args.iter().filter_map(|v| v.as_i64()).fold(1i64, |acc, x| acc * x).clamp(1, 128) as usize;
                        Ok(PyVal::Tensor { data: vec![0.0; n], kind })
                    }
                    "ones" => {
                        let n = args.iter().filter_map(|v| v.as_i64()).fold(1i64, |acc, x| acc * x).clamp(1, 128) as usize;
                        Ok(PyVal::Tensor { data: vec![1.0; n], kind })
                    }
                    "randn" => {
                        let n = args.iter().filter_map(|v| v.as_i64()).fold(1i64, |acc, x| acc * x).clamp(1, 128) as usize;
                        let mut data = Vec::with_capacity(n);
                        for _ in 0..n { data.push(self.next_randn()); }
                        Ok(PyVal::Tensor { data, kind })
                    }
                    "relu" => {
                        let a = extract_floats(args.first())?;
                        Ok(PyVal::Tensor { data: a.into_iter().map(|x| if x > 0.0 { x } else { 0.0 }).collect(), kind })
                    }
                    "dot" | "matmul" => {
                        let a = extract_floats(args.first())?;
                        let b = extract_floats(args.get(1))?;
                        let sum: f64 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
                        Ok(PyVal::Float(sum))
                    }
                    _ => Err(format!("AttributeError: module 'torch' has no attribute '{}'", method)),
                }
            }
            "math" => {
                let x = args.first().and_then(|v| v.as_f64()).ok_or_else(|| String::from("TypeError: math function expects number"))?;
                match method {
                    "sqrt" => Ok(PyVal::Float(libm_sqrt(x))),
                    "pow" => {
                        let y = args.get(1).and_then(|v| v.as_f64()).unwrap_or(1.0);
                        Ok(PyVal::Float(libm_pow(x, y)))
                    }
                    "floor" => Ok(PyVal::Int(if x >= 0.0 || (x as i64 as f64) == x { x as i64 } else { (x as i64) - 1 })),
                    "ceil" => Ok(PyVal::Int(if x <= 0.0 || (x as i64 as f64) == x { x as i64 } else { (x as i64) + 1 })),
                    "fabs" => Ok(PyVal::Float(x.abs())),
                    _ => Err(format!("AttributeError: module 'math' has no attribute '{}'", method)),
                }
            }
            "random" => match method {
                "random" => Ok(PyVal::Float(self.next_rand())),
                "randint" => {
                    let a = args.first().and_then(|v| v.as_i64()).unwrap_or(0);
                    let b = args.get(1).and_then(|v| v.as_i64()).unwrap_or(10);
                    let span = (b - a + 1).max(1) as f64;
                    Ok(PyVal::Int(a + (self.next_rand() * span) as i64))
                }
                "choice" => {
                    if let Some(PyVal::List(v)) = args.first() {
                        if v.is_empty() { return Err(String::from("IndexError: cannot choose from an empty sequence")); }
                        let idx = (self.next_rand() * v.len() as f64) as usize % v.len();
                        Ok(v[idx].clone())
                    } else {
                        Err(String::from("TypeError: choice() expects a sequence"))
                    }
                }
                _ => Err(format!("AttributeError: module 'random' has no attribute '{}'", method)),
            },
            "os" => match method {
                "getcwd" => Ok(PyVal::Str(String::from("/home/swapnil"))),
                "listdir" => Ok(PyVal::List(vec![
                    PyVal::Str(String::from("notes.txt")),
                    PyVal::Str(String::from("train_ai.py")),
                    PyVal::Str(String::from("todo.md")),
                    PyVal::Str(String::from(".venv")),
                ])),
                _ => Err(format!("AttributeError: module 'os' has no attribute '{}'", method)),
            },
            "zenkai" => {
                if method == "PageSimulator" {
                    return self.eval_call("PageSimulator", args, kw, services);
                }
                Ok(PyVal::Str(format!("zenkai.{}(ok)", method)))
            }
            _ => Ok(PyVal::Str(format!("{}.{}() executed in .venv", base, method))),
        }
    }

    fn eval_method_call(&self, mut obj: PyVal, method: &str, args: &[PyVal]) -> Result<(PyVal, PyVal), String> {
        match (&mut obj, method) {
            (PyVal::Str(s), "upper") => Ok((PyVal::None, PyVal::Str(s.to_uppercase()))),
            (PyVal::Str(s), "lower") => Ok((PyVal::None, PyVal::Str(s.to_lowercase()))),
            (PyVal::Str(s), "strip") => Ok((PyVal::None, PyVal::Str(String::from(s.trim())))),
            (PyVal::Str(s), "split") => {
                let sep = args.first().map(|v| v.display_str());
                let parts: Vec<PyVal> = match sep.as_deref() {
                    Some(d) if !d.is_empty() => s.split(d).map(|p| PyVal::Str(String::from(p))).collect(),
                    _ => s.split_whitespace().map(|p| PyVal::Str(String::from(p))).collect(),
                };
                Ok((PyVal::None, PyVal::List(parts)))
            }
            (PyVal::List(v), "append") => {
                let item = args.first().cloned().unwrap_or(PyVal::None);
                v.push(item);
                Ok((PyVal::List(v.clone()), PyVal::None))
            }
            (PyVal::List(v), "pop") => {
                let popped = v.pop().unwrap_or(PyVal::None);
                Ok((PyVal::List(v.clone()), popped))
            }
            (PyVal::Tensor { data, .. }, "mean") => {
                let m = if data.is_empty() { 0.0 } else { data.iter().sum::<f64>() / data.len() as f64 };
                Ok((PyVal::None, PyVal::Float(m)))
            }
            (PyVal::Tensor { data, .. }, "sum") => Ok((PyVal::None, PyVal::Float(data.iter().sum()))),
            (PyVal::Tensor { data, .. }, "tolist") => {
                let items = data.iter().map(|&x| PyVal::Float(x)).collect();
                Ok((PyVal::None, PyVal::List(items)))
            }
            (PyVal::Module(m), "step") if m.starts_with("zenkai") => Ok((PyVal::None, PyVal::Float(0.142))),
            _ => Err(format!("AttributeError: '{}' object has no attribute '{}'", obj.type_name(), method)),
        }
    }

    fn apply_binop(&self, l: PyVal, op: &str, r: PyVal) -> Result<PyVal, String> {
        match (l, op, r) {
            (PyVal::Int(a), "+", PyVal::Int(b)) => Ok(PyVal::Int(a.saturating_add(b))),
            (PyVal::Int(a), "-", PyVal::Int(b)) => Ok(PyVal::Int(a.saturating_sub(b))),
            (PyVal::Int(a), "*", PyVal::Int(b)) => Ok(PyVal::Int(a.saturating_mul(b))),
            (PyVal::Int(a), "//", PyVal::Int(b)) => {
                if b == 0 { Err(String::from("ZeroDivisionError: integer division by zero")) } else { Ok(PyVal::Int(a / b)) }
            }
            (PyVal::Int(a), "%", PyVal::Int(b)) => {
                if b == 0 { Err(String::from("ZeroDivisionError: integer modulo by zero")) } else { Ok(PyVal::Int(a % b)) }
            }
            (PyVal::Int(a), "**", PyVal::Int(b)) if b >= 0 => Ok(PyVal::Int(a.saturating_pow(b as u32))),
            (PyVal::Str(a), "+", PyVal::Str(b)) => Ok(PyVal::Str(format!("{}{}", a, b))),
            (PyVal::Str(a), "*", PyVal::Int(n)) | (PyVal::Int(n), "*", PyVal::Str(a)) => {
                let mut out = String::new();
                for _ in 0..n.clamp(0, 64) { out.push_str(&a); }
                Ok(PyVal::Str(out))
            }
            (PyVal::List(mut a), "+", PyVal::List(b)) => {
                a.extend(b);
                Ok(PyVal::List(a))
            }
            (PyVal::Tensor { data: a, kind }, op_s, PyVal::Tensor { data: b, .. }) => {
                let res: Vec<f64> = a.iter().zip(b.iter()).map(|(x, y)| match op_s {
                    "+" => x + y,
                    "-" => x - y,
                    "*" => x * y,
                    "/" => if *y == 0.0 { 0.0 } else { x / y },
                    _ => x + y,
                }).collect();
                Ok(PyVal::Tensor { data: res, kind })
            }
            (PyVal::Tensor { data, kind }, op_s, s) | (s, op_s, PyVal::Tensor { data, kind })
                if s.as_f64().is_some() =>
            {
                let k = s.as_f64().unwrap();
                let res: Vec<f64> = data.iter().map(|&x| match op_s {
                    "+" => x + k,
                    "-" => x - k,
                    "*" => x * k,
                    "/" => if k == 0.0 { 0.0 } else { x / k },
                    _ => x,
                }).collect();
                Ok(PyVal::Tensor { data: res, kind })
            }
            (a, op_s, b) => {
                let af = a.as_f64().ok_or_else(|| format!("TypeError: unsupported operand(s) for {}: '{}'", op_s, a.type_name()))?;
                let bf = b.as_f64().ok_or_else(|| format!("TypeError: unsupported operand(s) for {}: '{}'", op_s, b.type_name()))?;
                match op_s {
                    "+" => Ok(PyVal::Float(af + bf)),
                    "-" => Ok(PyVal::Float(af - bf)),
                    "*" => Ok(PyVal::Float(af * bf)),
                    "/" => if bf == 0.0 { Err(String::from("ZeroDivisionError: float division by zero")) } else { Ok(PyVal::Float(af / bf)) },
                    "//" => if bf == 0.0 { Err(String::from("ZeroDivisionError: division by zero")) } else { Ok(PyVal::Float((af / bf) as i64 as f64)) },
                    "%" => if bf == 0.0 { Err(String::from("ZeroDivisionError: modulo by zero")) } else { Ok(PyVal::Float(af % bf)) },
                    "**" => Ok(PyVal::Float(libm_pow(af, bf))),
                    _ => Err(format!("SyntaxError: unknown operator '{}'", op_s)),
                }
            }
        }
    }
}

fn libm_sqrt(x: f64) -> f64 {
    if x <= 0.0 {
        return 0.0;
    }
    let mut g = x;
    for _ in 0..16 {
        g = 0.5 * (g + x / g);
    }
    g
}

fn libm_pow(base: f64, exp: f64) -> f64 {
    let e = exp as i32;
    let mut res = 1.0f64;
    for _ in 0..e.abs().min(64) {
        res *= base;
    }
    if e < 0 && res != 0.0 { 1.0 / res } else { res }
}

fn extract_floats(v: Option<&PyVal>) -> Result<Vec<f64>, String> {
    match v {
        Some(PyVal::List(items)) => {
            let mut out = Vec::with_capacity(items.len());
            for it in items {
                match it {
                    PyVal::List(sub) => {
                        for s in sub {
                            out.push(s.as_f64().unwrap_or(0.0));
                        }
                    }
                    _ => out.push(it.as_f64().unwrap_or(0.0)),
                }
            }
            Ok(out)
        }
        Some(PyVal::Tensor { data, .. }) => Ok(data.clone()),
        Some(other) => other.as_f64().map(|x| vec![x]).ok_or_else(|| format!("TypeError: expected sequence, got {}", other.type_name())),
        None => Ok(Vec::new()),
    }
}

fn compare_vals(l: &PyVal, op: &str, r: &PyVal) -> Result<bool, String> {
    if let (Some(a), Some(b)) = (l.as_f64(), r.as_f64()) {
        return Ok(match op {
            "==" => (a - b).abs() < 1e-12,
            "!=" => (a - b).abs() >= 1e-12,
            "<" => a < b,
            "<=" => a <= b,
            ">" => a > b,
            ">=" => a >= b,
            _ => false,
        });
    }
    match (l, r) {
        (PyVal::Str(a), PyVal::Str(b)) => Ok(match op {
            "==" => a == b,
            "!=" => a != b,
            "<" => a < b,
            "<=" => a <= b,
            ">" => a > b,
            ">=" => a >= b,
            _ => false,
        }),
        _ => Ok(false),
    }
}

fn eval_index(target: &PyVal, idx_val: &PyVal) -> Result<PyVal, String> {
    match (target, idx_val) {
        (PyVal::List(v), i) => {
            let idx = i.as_i64().ok_or_else(|| String::from("TypeError: list indices must be integers"))?;
            let real = if idx < 0 { v.len() as i64 + idx } else { idx };
            v.get(real as usize).cloned().ok_or_else(|| String::from("IndexError: list index out of range"))
        }
        (PyVal::Tensor { data, .. }, i) => {
            let idx = i.as_i64().ok_or_else(|| String::from("TypeError: tensor indices must be integers"))?;
            let real = if idx < 0 { data.len() as i64 + idx } else { idx };
            data.get(real as usize).map(|&x| PyVal::Float(x)).ok_or_else(|| String::from("IndexError: index out of range"))
        }
        (PyVal::Str(s), i) => {
            let idx = i.as_i64().ok_or_else(|| String::from("TypeError: string indices must be integers"))?;
            let chars: Vec<char> = s.chars().collect();
            let real = if idx < 0 { chars.len() as i64 + idx } else { idx };
            chars.get(real as usize).map(|&c| PyVal::Str(format!("{}", c))).ok_or_else(|| String::from("IndexError: string index out of range"))
        }
        (PyVal::Dict(d), k) => {
            let ks = k.display_str();
            d.iter().find(|(dk, _)| dk == &ks).map(|(_, v)| v.clone()).ok_or_else(|| format!("KeyError: '{}'", ks))
        }
        _ => Err(format!("TypeError: '{}' object is not subscriptable", target.type_name())),
    }
}

fn strip_comment(line: &str) -> &str {
    let mut in_sq = false;
    let mut in_dq = false;
    for (i, b) in line.bytes().enumerate() {
        if b == b'\'' && !in_dq { in_sq = !in_sq; }
        else if b == b'"' && !in_sq { in_dq = !in_dq; }
        else if b == b'#' && !in_sq && !in_dq { return &line[..i]; }
    }
    line
}

fn leading_spaces(line: &str) -> usize {
    line.chars().take_while(|&c| c == ' ' || c == '\t').count()
}

fn collect_block(lines: &[String], start: usize, parent_indent: usize) -> (Vec<String>, usize) {
    let mut body = Vec::new();
    let mut i = start;
    while i < lines.len() {
        let raw = &lines[i];
        let t = strip_comment(raw).trim();
        if t.is_empty() {
            i += 1;
            continue;
        }
        if leading_spaces(raw) <= parent_indent {
            break;
        }
        body.push(raw.clone());
        i += 1;
    }
    (body, i)
}

fn split_top_level(s: &str, sep: char) -> Vec<&str> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut in_sq = false;
    let mut in_dq = false;
    let mut start = 0;
    for (i, ch) in s.char_indices() {
        if ch == '\'' && !in_dq { in_sq = !in_sq; }
        else if ch == '"' && !in_sq { in_dq = !in_dq; }
        else if !in_sq && !in_dq {
            if matches!(ch, '(' | '[' | '{') { depth += 1; }
            else if matches!(ch, ')' | ']' | '}') { depth -= 1; }
            else if depth == 0 && ch == sep {
                out.push(&s[start..i]);
                start = i + ch.len_utf8();
            }
        }
    }
    out.push(&s[start..]);
    out
}

fn find_top_level(s: &str, pat: &str) -> Option<usize> {
    let mut depth = 0i32;
    let mut in_sq = false;
    let mut in_dq = false;
    let bytes = s.as_bytes();
    for i in 0..bytes.len() {
        let b = bytes[i];
        if b == b'\'' && !in_dq { in_sq = !in_sq; }
        else if b == b'"' && !in_sq { in_dq = !in_dq; }
        else if !in_sq && !in_dq {
            if matches!(b, b'(' | b'[' | b'{') { depth += 1; }
            else if matches!(b, b')' | b']' | b'}') { depth -= 1; }
            else if depth == 0 && s[i..].starts_with(pat) {
                return Some(i);
            }
        }
    }
    None
}

fn find_top_level_cmp(s: &str, op: &str) -> Option<usize> {
    let mut depth = 0i32;
    let mut in_sq = false;
    let mut in_dq = false;
    let bytes = s.as_bytes();
    for i in 0..bytes.len() {
        let b = bytes[i];
        if b == b'\'' && !in_dq { in_sq = !in_sq; }
        else if b == b'"' && !in_sq { in_dq = !in_dq; }
        else if !in_sq && !in_dq {
            if matches!(b, b'(' | b'[' | b'{') { depth += 1; }
            else if matches!(b, b')' | b']' | b'}') { depth -= 1; }
            else if depth == 0 && s[i..].starts_with(op) {
                if (op == "<" || op == ">") && i + 1 < bytes.len() && bytes[i + 1] == b'=' {
                    continue;
                }
                return Some(i);
            }
        }
    }
    None
}

fn find_top_level_kw(s: &str, kw: &str) -> Option<usize> {
    let mut depth = 0i32;
    let mut in_sq = false;
    let mut in_dq = false;
    let bytes = s.as_bytes();
    for i in 0..bytes.len() {
        let b = bytes[i];
        if b == b'\'' && !in_dq { in_sq = !in_sq; }
        else if b == b'"' && !in_sq { in_dq = !in_dq; }
        else if !in_sq && !in_dq {
            if matches!(b, b'(' | b'[' | b'{') { depth += 1; }
            else if matches!(b, b')' | b']' | b'}') { depth -= 1; }
            else if depth == 0 && s[i..].starts_with(kw) {
                let left_ok = i == 0 || bytes[i - 1].is_ascii_whitespace();
                let right_ok = i + kw.len() >= bytes.len() || bytes[i + kw.len()].is_ascii_whitespace();
                if left_ok && right_ok {
                    return Some(i);
                }
            }
        }
    }
    None
}

fn find_rightmost_addsub(s: &str) -> Option<(usize, &'static str)> {
    let mut depth = 0i32;
    let mut in_sq = false;
    let mut in_dq = false;
    let bytes = s.as_bytes();
    let mut found = None;
    for i in 0..bytes.len() {
        let b = bytes[i];
        if b == b'\'' && !in_dq { in_sq = !in_sq; }
        else if b == b'"' && !in_sq { in_dq = !in_dq; }
        else if !in_sq && !in_dq {
            if matches!(b, b'(' | b'[' | b'{') { depth += 1; }
            else if matches!(b, b')' | b']' | b'}') { depth -= 1; }
            else if depth == 0 && (b == b'+' || b == b'-') && i > 0 {
                // Skip unary +/- after another operator
                let prev = bytes[..i].iter().rposition(|&c| !c.is_ascii_whitespace()).map(|idx| bytes[idx]);
                if !matches!(prev, Some(b'+' | b'-' | b'*' | b'/' | b'%' | b'(' | b'[' | b',' | b'=')) {
                    found = Some((i, if b == b'+' { "+" } else { "-" }));
                }
            }
        }
    }
    found
}

fn find_rightmost_muldiv(s: &str) -> Option<(usize, &'static str)> {
    let mut depth = 0i32;
    let mut in_sq = false;
    let mut in_dq = false;
    let bytes = s.as_bytes();
    let mut found = None;
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'\'' && !in_dq { in_sq = !in_sq; }
        else if b == b'"' && !in_sq { in_dq = !in_dq; }
        else if !in_sq && !in_dq {
            if matches!(b, b'(' | b'[' | b'{') { depth += 1; }
            else if matches!(b, b')' | b']' | b'}') { depth -= 1; }
            else if depth == 0 {
                if s[i..].starts_with("**") {
                    i += 2;
                    continue;
                } else if s[i..].starts_with("//") {
                    found = Some((i, "//"));
                    i += 2;
                    continue;
                } else if b == b'*' {
                    found = Some((i, "*"));
                } else if b == b'/' {
                    found = Some((i, "/"));
                } else if b == b'%' {
                    found = Some((i, "%"));
                }
            }
        }
        i += 1;
    }
    found
}

fn matching_paren_end(s: &str, open_idx: usize) -> Option<usize> {
    let mut depth = 0i32;
    let mut in_sq = false;
    let mut in_dq = false;
    for (i, b) in s.bytes().enumerate().skip(open_idx) {
        if b == b'\'' && !in_dq { in_sq = !in_sq; }
        else if b == b'"' && !in_sq { in_dq = !in_dq; }
        else if !in_sq && !in_dq {
            if b == b'(' { depth += 1; }
            else if b == b')' {
                depth -= 1;
                if depth == 0 { return Some(i); }
            }
        }
    }
    None
}

fn find_matching_open_paren(s: &str, close_idx: usize) -> Option<usize> {
    let mut depth = 0i32;
    let mut in_sq = false;
    let mut in_dq = false;
    let bytes = s.as_bytes();
    let mut i = close_idx as isize;
    while i >= 0 {
        let b = bytes[i as usize];
        if b == b'\'' && !in_dq { in_sq = !in_sq; }
        else if b == b'"' && !in_sq { in_dq = !in_dq; }
        else if !in_sq && !in_dq {
            if b == b')' { depth += 1; }
            else if b == b'(' {
                depth -= 1;
                if depth == 0 { return Some(i as usize); }
            }
        }
        i -= 1;
    }
    None
}

fn find_matching_open_bracket(s: &str, close_idx: usize) -> Option<usize> {
    let mut depth = 0i32;
    let mut in_sq = false;
    let mut in_dq = false;
    let bytes = s.as_bytes();
    let mut i = close_idx as isize;
    while i >= 0 {
        let b = bytes[i as usize];
        if b == b'\'' && !in_dq { in_sq = !in_sq; }
        else if b == b'"' && !in_sq { in_dq = !in_dq; }
        else if !in_sq && !in_dq {
            if b == b']' { depth += 1; }
            else if b == b'[' {
                depth -= 1;
                if depth == 0 { return Some(i as usize); }
            }
        }
        i -= 1;
    }
    None
}

fn find_last_top_level_dot(s: &str) -> Option<usize> {
    let mut depth = 0i32;
    let mut in_sq = false;
    let mut in_dq = false;
    let mut found = None;
    for (i, b) in s.bytes().enumerate() {
        if b == b'\'' && !in_dq { in_sq = !in_sq; }
        else if b == b'"' && !in_sq { in_dq = !in_dq; }
        else if !in_sq && !in_dq {
            if matches!(b, b'(' | b'[' | b'{') { depth += 1; }
            else if matches!(b, b')' | b']' | b'}') { depth -= 1; }
            else if depth == 0 && b == b'.' {
                found = Some(i);
            }
        }
    }
    found
}
