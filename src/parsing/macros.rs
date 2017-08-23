macro_rules! remove_overriden {
    (@remove_requires $rem_from:expr, $a:ident.$ov:ident) => {
        if let Some(ref ora) = $a.$ov {
            for i in (0 .. $rem_from.len()).rev() {
                let should_remove = ora.iter().any(|name| &name == &&$rem_from[i]);
                if should_remove { $rem_from.swap_remove(i); }
            }
        }
    };
    (@remove $rem_from:expr, $a:ident.$ov:ident) => {
        if let Some(ref ora) = $a.$ov {
            vec_remove_all!($rem_from, ora.iter());
        }
    };
    (@arg $parser:ident, $arg:ident) => {
        remove_overriden!(@remove_requires $parser.required, $arg.requires);
        // remove_overriden!(@remove $parser.conflicts, $arg.conflicts_with);
        remove_overriden!(@remove $parser.overrides, $arg.overrides_with);
    };
    ($parser:ident, $name:expr) => {
        debugln!("remove_overriden!;");
        if let Some(a) = args!($parser.app).find(|a| &a.name == $name) {
            remove_overriden!(@arg $parser, a);
        }
    };
}

macro_rules! arg_post_processing {
    ($parser:ident, $arg:ident, $matcher:ident) => {
        debugln!("arg_post_processing!:{};", $arg.name);
        // Handle POSIX overrides
        if $parser.overrides.contains(&$arg.name) {
            debugln!("arg_post_processing!:{}: Already in overrides", $arg.name);
            if let Some(ref name) = find_override!($parser.app, &$arg.name, $matcher) {
                $matcher.remove(name);
                remove_overriden!($parser, name);
            }
        }

        // Add overrides
        if let Some(ref or) = $arg.overrides_with {
            debug!("arg_post_processing!:{}: Has overrides", $arg.name);
            $matcher.remove_all(&*or);
            for pa in or { remove_overriden!($parser, pa); }
            $parser.overrides.extend(or);
            vec_remove_all!($parser.required, or.iter());
        }

        // Handle conflicts
        if let Some(ref bl) = $arg.conflicts_with {
            debugln!("arg_post_processing!:{}: Has conflicts", $arg.name);

            // for c in bl {
            //     // Inject two-way conflicts
            //     debugln!("arg_post_processing!:{}: adding conflict {:?}", $arg.name, c);
            //     // $parser.conflicts.push(c);
            //     // debug!("arg_post_processing!: Has '{}' already been matched...", c);
            //     // if $matcher.contains(c) {
            //     //     sdebugln!("Yes");
            //     //     $parser.conflicts.push(c);
            //     // } else {
            //     //     sdebugln!("No");
            //     // }
            // }

            // $parser.conflicts.extend_from_slice(&*bl);
            vec_remove_all!($parser.overrides, bl.iter());
            // vec_remove_all!($me.required, bl.iter());
        }

        // Add all required args which aren't already found in matcher to the master
        // list
        if let Some(ref reqs) = $arg.requires {
            debugln!("arg_post_processing!:{}: Has requirements", $arg.name);
            for n in reqs.iter()
                .filter(|req| !$matcher.contains(&req))
                .map(|&name| name) {
                    
                $parser.required.push(n);
            }
        }
        if let Some(ref reqs) = $arg.requires {
            debugln!("arg_post_processing!:{}: Has conditional requirements", $arg.name);
            for n in reqs.iter()
                .filter(|req| !$matcher.contains(&req))
                .map(|&name| name) {
                    
                $parser.required.push(n);
            }
        }

        handle_group_reqs!($parser, $arg);
    };
}

macro_rules! handle_group_reqs {
    ($parser:ident, $arg:ident) => ({
        debugln!("handle_group_reqs!:{};", $arg.name);
        for grp in &$parser.app.groups {
            let found = if grp.args.contains(&$arg.name) {
                if let Some(ref reqs) = grp.requires {
                    debugln!("handle_group_reqs!:{}: Adding {:?} to the required list", $arg.name, reqs);
                    $parser.required.extend(reqs);
                }
                // if let Some(ref bl) = grp.conflicts {
                //     $parser.conflicts.extend(bl);
                // }
                true // @VERIFY What if arg is in more than one group with different reqs?
            } else {
                false
            };
            if found {
                debugln!("handle_group_reqs!:{}:iter:{}: found in group", $arg.name, grp.name);
                // Removes args in this group from the requried list because this group has now
                // been matched.
                // @VERIFY how does this interact if one of those args is required for other
                // reasons?
                for i in (0 .. $parser.required.len()).rev() {
                    let should_remove = grp.args.contains(&$parser.required[i]);
                    if should_remove { $parser.required.swap_remove(i); }
                }
                // debugln!("handle_group_reqs!:{}:iter:{}: Adding {:?} to conflicts", $arg.name, grp.name, grp.args);
                // if !grp.multiple {
                    // $parser.conflicts.extend(&grp.args);
                    // debugln!("handle_group_reqs!:{}:{}: removing from conflicts", $arg.name, grp.name);
                    // for i in (0 .. $parser.conflicts.len()).rev() {
                        // let should_remove = $parser.conflicts[i] == $arg.name;
                        // if should_remove { $parser.conflicts.swap_remove(i); }
                    // }
                // }
            }
        }
    })
}

macro_rules! parse_positional {
    (
        $parser:ident, 
        $p:ident,
        $arg_os:ident,
        $pos_counter:ident,
        $matcher:ident
    ) => {
        debugln!("parse_positional!;");

        if !$parser.is_set(AS::TrailingValues) &&
           ($parser.is_set(AS::TrailingVarArg) &&
            $pos_counter == positionals!($parser.app).count()) {
            $parser.app._settings.set(AS::TrailingValues);
        }
        let _ = try!($parser.add_val_to_arg($p, &$arg_os, $matcher));

        $matcher.inc_occurrence_of($p.name);
        let _ = $parser.groups_for_arg($p.name)
                      .and_then(|vec| Some($matcher.inc_occurrences_of(&*vec)));
        if $parser.cache.map_or(true, |name| name != $p.name) {
            arg_post_processing!($parser, $p, $matcher);
            $parser.cache = Some($p.name);
        }

        $parser.app._settings.set(AS::ValidArgFound);
        // Only increment the positional counter if it doesn't allow multiples
        if !$p.is_set(ArgSettings::Multiple) {
            $pos_counter += 1;
        }
    };
}
