// Additive pre-implementation controls for docs/CSSOM_METHODS.md.
// Included in boa.rs's existing private tests; no production hooks or new limits.
#[cfg(test)]
mod cssom_methods_resource_controls {
    use super::*;

    const HOST_LIMIT: usize = super::super::super::MAX_DOM_BYTES;

    fn setup() -> &'static str {
        r#"
        var methodsNode=document.getElementById('out');
        methodsNode.setAttribute('style','color:red; width:7px');
        var methodsStyle=methodsNode.style;
        var methodsSet=methodsStyle.setProperty;
        var methodsRemove=methodsStyle.removeProperty;
        var methodsPriority=methodsStyle.getPropertyPriority;
        var methodsGet=methodsStyle.getPropertyValue;
        var methodsCaught=0, methodsTouched=0, methodsContinued=false;
        function methodsTypeError(callback) {
            try { callback(); }
            catch(error) {
                if(!(error instanceof TypeError)) throw error;
                methodsCaught++;
                return;
            }
            throw new Error('expected an actual TypeError');
        }
        "#
    }

    fn work_unchanged(realm: &BoaPageRealm) {
        let report = realm.engine.stats();
        assert_eq!(
            (
                report.jobs_admitted,
                report.jobs_executed,
                report.pending_jobs
            ),
            (2, 2, 0),
            "synchronous methods must retain both real lifecycle task costs"
        );
        assert_eq!(realm.lifecycle.phase, LifecyclePhase::LoadComplete);
        let data = realm.state.borrow();
        assert!(data.timers.is_empty());
        assert!(data.dom_tasks.is_empty());
        assert!(data.rejections.is_empty());
    }

    fn checked(realm: &mut BoaPageRealm, source: &str) {
        let before = realm.engine.stats();
        evaluate(realm, source);
        let after = realm.engine.stats();
        after.validate_after(&before).unwrap();
        assert_eq!(after.source_admissions, before.source_admissions + 1);
        assert_eq!(
            after.source_bytes,
            before.source_bytes + source.len().max(1)
        );
        work_unchanged(realm);
    }

    fn owned() -> BoaPageRealm {
        let mut realm = realm(setup());
        // This must fail on actual old production before quota is manipulated.
        // Catching a later call to undefined is not a resource negative proof.
        checked(
            &mut realm,
            r#"
            if(typeof methodsSet!=='function') throw 'missing setProperty';
            if(typeof methodsRemove!=='function') throw 'missing removeProperty';
            if(typeof methodsPriority!=='function') throw 'missing getPropertyPriority';
            if(typeof methodsGet!=='function') throw 'missing getPropertyValue';
            "#,
        );
        realm
    }

    fn attributes(realm: &BoaPageRealm) -> Vec<(String, String)> {
        let data = realm.state.borrow();
        let id = data
            .host
            .document
            .query_selector(0, "#out")
            .unwrap()
            .unwrap();
        data.host.document.nodes[id].attributes.clone()
    }

    fn number(realm: &mut BoaPageRealm, name: &str) -> f64 {
        realm.engine.get_global(name).unwrap().as_number().unwrap()
    }

    fn continued(realm: &mut BoaPageRealm) {
        assert!(
            realm
                .engine
                .get_global("methodsContinued")
                .unwrap()
                .as_boolean()
                .unwrap()
        );
        assert!(!realm.engine.is_fatal());
        work_unchanged(realm);
    }

    #[test]
    fn style_method_metadata_and_cached_functions_survive_normal_checkpoint_collection() {
        let mut realm = owned();
        let methods = realm.state.borrow().methods.len();
        collect(&mut realm);
        checked(
            &mut realm,
            r#"
            var functions=[methodsSet,methodsRemove,methodsPriority,methodsGet];
            var names=['setProperty','removeProperty','getPropertyPriority','getPropertyValue'];
            var lengths=[2,1,1,1];
            for(var i=0;i<functions.length;i++) {
                if(functions[i].name!==names[i] || functions[i].length!==lengths[i])
                    throw 'wrong style method metadata';
                if(methodsStyle[names[i]]!==functions[i]) throw 'method identity changed';
            }
            methodsSet.call(methodsStyle,'width','9px');
            if(methodsGet.call(methodsStyle,'width')!=='9px') throw 'collected native method';
            methodsContinued=true;
            "#,
        );
        assert_eq!(realm.state.borrow().methods.len(), methods);
        continued(&mut realm);
    }

    #[test]
    fn method_has_is_structural_with_cold_method_cache_at_exhausted_host_quota() {
        // Prove genuine new APIs first, in a separate ordinary realm, so the
        // actual presence checks below still exercise uncached methods.
        drop(owned());
        let mut realm = realm(
            r#"
            var methodsNode=document.getElementById('out');
            methodsNode.setAttribute('style','--x:'+'a'.repeat(16380));
            var methodsStyle=methodsNode.style;
            var methodsContinued=false;
            "#,
        );
        let before = attributes(&realm);
        let methods = realm.state.borrow().methods.len();
        let wrappers = realm.state.borrow().wrappers_created;
        realm.state.borrow_mut().host.allocated = HOST_LIMIT;
        checked(
            &mut realm,
            r#"
            for(var i=0;i<16;i++) {
                if(!Reflect.has(methodsStyle,'setProperty') ||
                   !Reflect.has(methodsStyle,'removeProperty') ||
                   !Reflect.has(methodsStyle,'getPropertyPriority') ||
                   !Reflect.has(methodsStyle,'getPropertyValue') ||
                   !Reflect.has(methodsStyle,'cssText') ||
                   !Reflect.has(methodsStyle,'width')) throw 'missing structural member';
                if(Reflect.has(methodsStyle,'unknownCssMethod') ||
                   Reflect.has(methodsStyle,'--custom')) throw 'invented named member';
            }
            methodsContinued=true;
            "#,
        );
        assert_eq!(realm.state.borrow().host.allocated, HOST_LIMIT);
        assert_eq!(realm.state.borrow().methods.len(), methods);
        assert_eq!(realm.state.borrow().wrappers_created, wrappers);
        assert_eq!(attributes(&realm), before);
        continued(&mut realm);
        // Exact raw input is admitted, but canonical cssText would exceed the
        // attribute bound. Presence must not invoke that parser/getter either.
    }

    #[test]
    fn method_receiver_and_required_arity_checks_precede_all_argument_coercion() {
        let mut realm = owned();
        let before = attributes(&realm);
        let admitted = realm.state.borrow().host.allocated;
        checked(
            &mut realm,
            r#"
            var name={toString(){methodsTouched++;return 'color';}};
            var value={toString(){methodsTouched++;return 'blue';}};
            var priority={toString(){methodsTouched++;return 'important';}};
            var hostile=new Proxy(methodsStyle,{get(){methodsTouched++;throw 'receiver getter';}});
            var bad=[undefined,null,{},Object.create(methodsStyle),hostile,methodsNode,document,window];
            for(var receiver of bad) {
                methodsTypeError(()=>methodsSet.call(receiver,name,value,priority));
                methodsTypeError(()=>methodsRemove.call(receiver,name));
                methodsTypeError(()=>methodsPriority.call(receiver,name));
            }
            methodsTypeError(()=>methodsSet.call(methodsStyle));
            methodsTypeError(()=>methodsSet.call(methodsStyle,name));
            methodsTypeError(()=>methodsRemove.call(methodsStyle));
            methodsTypeError(()=>methodsPriority.call(methodsStyle));
            methodsContinued=true;
            "#,
        );
        assert_eq!(number(&mut realm, "methodsCaught"), 28.0);
        assert_eq!(number(&mut realm, "methodsTouched"), 0.0);
        assert_eq!(realm.state.borrow().host.allocated, admitted);
        assert_eq!(attributes(&realm), before);
        continued(&mut realm);
    }

    #[test]
    fn local_methods_reject_live_and_stale_foreign_style_without_old_context_access() {
        let mut first = owned();
        let foreign = first.engine.get_global("methodsStyle").unwrap();
        let first_before = attributes(&first);
        let mut second = owned();
        second
            .engine
            .context_mut()
            .register_global_property(JsString::from("foreignStyle"), foreign, Attribute::all())
            .unwrap();
        let before = attributes(&second);
        let admitted = second.state.borrow().host.allocated;
        let check_foreign = r#"
            methodsCaught=0;
            var name={toString(){methodsTouched++;return 'color';}};
            var value={toString(){methodsTouched++;return 'blue';}};
            methodsTypeError(()=>methodsSet.call(foreignStyle,name,value));
            methodsTypeError(()=>methodsRemove.call(foreignStyle,name));
            methodsTypeError(()=>methodsPriority.call(foreignStyle,name));
            methodsContinued=true;
        "#;
        checked(&mut second, check_foreign);
        assert_eq!(number(&mut second, "methodsCaught"), 3.0);
        assert_eq!(number(&mut second, "methodsTouched"), 0.0);
        assert_eq!(attributes(&first), first_before);
        drop(first);
        modern::boa_gc::force_collect();
        checked(&mut second, check_foreign);
        assert_eq!(number(&mut second, "methodsCaught"), 3.0);
        assert_eq!(number(&mut second, "methodsTouched"), 0.0);
        assert_eq!(second.state.borrow().host.allocated, admitted);
        assert_eq!(attributes(&second), before);
        continued(&mut second);
    }

    #[test]
    fn all_set_arguments_convert_before_unknown_property_or_invalid_priority_semantics() {
        let mut realm = owned();
        let before = attributes(&realm);
        let admitted = realm.state.borrow().host.allocated;
        checked(
            &mut realm,
            r#"
            var order='';
            function argument(letter,text) {
                return {toString(){order+=letter;return text;}};
            }
            if(methodsSet.call(methodsStyle,argument('N','unknown-property'),
                    argument('V','blue'),argument('P','invalid'))!==undefined)
                throw 'setProperty did not return undefined';
            if(order!=='NVP') throw 'unknown property skipped conversion';
            order='';
            methodsSet.call(methodsStyle,argument('N','color'),argument('V','blue'),
                argument('P','important '));
            if(order!=='NVP') throw 'invalid priority skipped conversion';
            order='';
            methodsTypeError(()=>methodsSet.call(methodsStyle,argument('N','x'.repeat(16385)),
                argument('V','blue'),argument('P','important')));
            if(order!=='NVP') throw 'name bound skipped later conversion';
            methodsContinued=true;
            "#,
        );
        assert_eq!(realm.state.borrow().host.allocated, admitted);
        assert_eq!(attributes(&realm), before);
        continued(&mut realm);
    }

    #[test]
    fn ordinary_conversion_throw_keeps_identity_inner_mutation_and_only_completed_arguments() {
        for stop in 0..3 {
            let mut realm = owned();
            let source = format!(
                r#"
                var order='', original={{}};
                function argument(index,letter,text) {{
                    return {{toString(){{
                        order+=letter;
                        if(index==={stop}) {{ methodsStyle.width='11px'; throw original; }}
                        return text;
                    }}}};
                }}
                try {{ methodsSet.call(methodsStyle,argument(0,'N','color'),
                        argument(1,'V','blue'),argument(2,'P','important')); }}
                catch(error) {{ if(error!==original) throw 'lost thrown identity'; methodsCaught++; }}
                if(order!==['N','NV','NVP'][{stop}]) throw 'conversion continued after throw';
                if(methodsGet.call(methodsStyle,'width')!=='11px' ||
                   methodsGet.call(methodsStyle,'color')!=='red') throw 'lost inner or applied outer mutation';
                methodsContinued=true;
                "#
            );
            checked(&mut realm, &source);
            assert_eq!(number(&mut realm, "methodsCaught"), 1.0);
            continued(&mut realm);
        }
        for method in ["methodsRemove", "methodsPriority"] {
            let mut realm = owned();
            let source = format!(
                r#"
                var original={{}};
                var name={{toString(){{methodsTouched++;methodsStyle.width='11px';throw original;}}}};
                try {{ {method}.call(methodsStyle,name); }}
                catch(error) {{ if(error!==original) throw 'lost thrown identity'; methodsCaught++; }}
                if(methodsGet.call(methodsStyle,'width')!=='11px' ||
                   methodsGet.call(methodsStyle,'color')!=='red') throw 'lost inner or applied outer mutation';
                methodsContinued=true;
                "#
            );
            checked(&mut realm, &source);
            assert_eq!(number(&mut realm, "methodsCaught"), 1.0);
            assert_eq!(number(&mut realm, "methodsTouched"), 1.0);
            continued(&mut realm);
        }
    }

    #[test]
    fn successful_reentry_reads_post_conversion_detached_owner_state() {
        let mut realm = owned();
        checked(
            &mut realm,
            r#"
            var order='';
            methodsSet.call(methodsStyle,
                {toString(){order+='N';methodsStyle.width='11px';return 'color';}},
                {toString(){order+='V';methodsStyle.height='13px';return 'blue';}},
                {toString(){order+='P';document.body.removeChild(methodsNode);return 'important';}});
            if(order!=='NVP') throw 'conversion order changed';
            if(methodsGet.call(methodsStyle,'width')!=='11px' ||
               methodsGet.call(methodsStyle,'height')!=='13px' ||
               methodsGet.call(methodsStyle,'color')!=='blue' ||
               methodsPriority.call(methodsStyle,'color')!=='important') throw 'stale outer read';
            methodsContinued=true;
            "#,
        );
        continued(&mut realm);
    }

    #[test]
    fn priority_result_quota_failure_is_ordinary_unreturned_and_unrefunded() {
        let mut realm = owned();
        checked(
            &mut realm,
            "methodsNode.setAttribute('style','color:red!important; width:7px');",
        );
        let before = attributes(&realm);
        realm.state.borrow_mut().host.allocated = HOST_LIMIT;
        checked(
            &mut realm,
            r#"
            var returned=false;
            methodsTypeError(()=>{methodsPriority.call(methodsStyle,'color');returned=true;});
            if(returned) throw 'quota failure returned a priority';
            methodsContinued=true;
            "#,
        );
        assert_eq!(number(&mut realm, "methodsCaught"), 1.0);
        assert!(realm.state.borrow().host.allocated > HOST_LIMIT);
        assert_eq!(attributes(&realm), before);
        continued(&mut realm);
    }

    #[test]
    fn removal_return_admission_fails_before_an_otherwise_admissible_empty_attribute_commit() {
        let mut realm = owned();
        checked(&mut realm, "methodsNode.setAttribute('style','width:7px');");
        let before = attributes(&realm);
        // Removing the sole declaration serializes an empty block. Existing
        // attr admission alone costs exactly "style".len() == 5 bytes and fits.
        // Returning "7px" needs at least 3 UTF-16 units == 6 bytes, before any
        // actual serializer capacity. Thus the result cannot fit; no allocator
        // String-capacity guess or pre-run measurement is needed for this edge.
        assert_eq!("style".len(), 5);
        assert!("7px".encode_utf16().count() * std::mem::size_of::<u16>() > 5);
        realm.state.borrow_mut().host.allocated = HOST_LIMIT - "style".len();
        checked(
            &mut realm,
            r#"
            var returned=false;
            methodsTypeError(()=>{methodsRemove.call(methodsStyle,'width');returned=true;});
            if(returned) throw 'unadmitted removal result returned';
            methodsContinued=true;
            "#,
        );
        assert_eq!(number(&mut realm, "methodsCaught"), 1.0);
        assert!(realm.state.borrow().host.allocated > HOST_LIMIT);
        assert_eq!(
            attributes(&realm),
            before,
            "must not commit style='' before result admission"
        );
        continued(&mut realm);
    }

    #[test]
    fn set_and_partial_shorthand_removal_commit_failure_preserve_the_whole_attribute() {
        for operation in [
            "methodsSet.call(methodsStyle,'margin','2px','important')",
            "methodsRemove.call(methodsStyle,'margin')",
        ] {
            let mut realm = owned();
            checked(
                &mut realm,
                "methodsNode.setAttribute('style','margin-top:1px; width:7px');",
            );
            let before = attributes(&realm);
            // Partial margin has no shorthand serialization, but its component
            // still must be removed. Its empty return has no String/UTF-16 host
            // payload charge; the actual replacement attribute cannot fit.
            realm.state.borrow_mut().host.allocated = HOST_LIMIT;
            checked(
                &mut realm,
                &format!("methodsTypeError(()=>{{ {operation}; }}); methodsContinued=true;"),
            );
            assert_eq!(number(&mut realm, "methodsCaught"), 1.0, "{operation}");
            assert!(realm.state.borrow().host.allocated > HOST_LIMIT);
            assert_eq!(attributes(&realm), before, "{operation}");
            continued(&mut realm);
        }
    }

    #[test]
    fn genuine_noncompeting_noops_succeed_at_full_quota_without_raw_rewrite_or_attribute_creation()
    {
        let mut realm = owned();
        checked(
            &mut realm,
            r#"
            var absentNode=document.createElement('div');
            var absentStyle=absentNode.style;
            methodsNode.setAttribute('style','color:red;; width:7px; unknown:raw');
            "#,
        );
        let before = attributes(&realm);
        realm.state.borrow_mut().host.allocated = HOST_LIMIT;
        checked(
            &mut realm,
            r#"
            methodsSet.call(methodsStyle,'width','7px');
            methodsSet.call(methodsStyle,'unknown-property','blue');
            methodsSet.call(methodsStyle,'width','not-a-width');
            methodsSet.call(methodsStyle,'width','9px','important ');
            if(methodsRemove.call(methodsStyle,'height')!=='' ||
               methodsPriority.call(methodsStyle,'width')!=='' ||
               methodsPriority.call(methodsStyle,'unknown-property')!=='') throw 'wrong empty result';
            methodsSet.call(absentStyle,'width','', 'invalid');
            if(methodsRemove.call(absentStyle,'width')!=='' ||
               absentNode.getAttribute('style')!==null) throw 'no-op created style attribute';
            methodsContinued=true;
            "#,
        );
        assert_eq!(realm.state.borrow().host.allocated, HOST_LIMIT);
        assert_eq!(attributes(&realm), before);
        continued(&mut realm);
    }

    #[test]
    fn method_name_value_and_priority_bounds_include_invalid_unknown_and_removal_inputs() {
        let mut realm = owned();
        let before = attributes(&realm);
        checked(
            &mut realm,
            r#"
            var exact='x'.repeat(16384), tooLarge=exact+'x';
            methodsSet.call(methodsStyle,exact,'blue');
            methodsSet.call(methodsStyle,'width',exact);
            methodsSet.call(methodsStyle,'color','blue',exact);
            if(methodsPriority.call(methodsStyle,exact)!=='' ||
               methodsRemove.call(methodsStyle,exact)!=='') throw 'exact unknown name should be empty';
            methodsTypeError(()=>methodsSet.call(methodsStyle,tooLarge,'blue'));
            methodsTypeError(()=>methodsSet.call(methodsStyle,'unknown-property',tooLarge));
            methodsTypeError(()=>methodsSet.call(methodsStyle,'color','blue',tooLarge));
            methodsTypeError(()=>methodsSet.call(methodsStyle,'color','',tooLarge));
            methodsTypeError(()=>methodsRemove.call(methodsStyle,tooLarge));
            methodsTypeError(()=>methodsPriority.call(methodsStyle,tooLarge));
            // The existing limit is UTF-8 bytes, not JS UTF-16 string length.
            methodsSet.call(methodsStyle,'width','é'.repeat(8192));
            methodsTypeError(()=>methodsSet.call(methodsStyle,'width','é'.repeat(8193)));
            methodsContinued=true;
            "#,
        );
        assert_eq!(number(&mut realm, "methodsCaught"), 7.0);
        assert_eq!(attributes(&realm), before);
        continued(&mut realm);
    }

    #[test]
    fn canonical_expansion_failure_for_set_and_remove_keeps_exact_admitted_raw_attribute() {
        let mut realm = owned();
        checked(
            &mut realm,
            r#"
            var prefix='width:7px;';
            for(var i=0;i<16;i++) prefix+='--p'+i+':a;';
            prefix+='--tail:';
            var exactRaw=prefix+'b'.repeat(16384-prefix.length);
            if(exactRaw.length!==16384) throw 'wrong authored exact input';
            methodsNode.setAttribute('style',exactRaw);
            "#,
        );
        let before = attributes(&realm);
        // Each custom declaration needs the canonical colon-space, and the
        // final one needs a semicolon. Those >10 added bytes exceed the removed
        // width declaration's 10 bytes, so BOTH replacements exceed 16,384.
        checked(
            &mut realm,
            r#"
            methodsTypeError(()=>methodsSet.call(methodsStyle,'width','8px'));
            methodsTypeError(()=>methodsRemove.call(methodsStyle,'width'));
            methodsContinued=true;
            "#,
        );
        assert_eq!(number(&mut realm, "methodsCaught"), 2.0);
        assert_eq!(attributes(&realm), before);
        continued(&mut realm);
    }

    #[test]
    fn conversion_fatals_remain_unmaskable_without_undoing_completed_inner_mutation() {
        for body in [
            "function recurse(){return recurse();}return recurse();",
            "while(true){}",
        ] {
            for operation in [
                "methodsSet.call(methodsStyle,'color','blue',fatalArgument)",
                "methodsRemove.call(methodsStyle,fatalArgument)",
                "methodsPriority.call(methodsStyle,fatalArgument)",
            ] {
                let mut realm = owned();
                let before = realm.engine.stats();
                let source = format!(
                    "var fatalArgument={{toString(){{methodsStyle.width='11px';{body}}}}};try{{{operation};}}catch(error){{document.title='caught fatal';}}document.title='continued after fatal';"
                );
                assert!(
                    realm.engine.evaluate(&source).is_err(),
                    "{operation}: {body}"
                );
                assert!(realm.engine.is_fatal());
                assert_eq!(message_title(&realm), "");
                let style = attributes(&realm)
                    .into_iter()
                    .find(|(name, _)| name == "style")
                    .unwrap()
                    .1;
                assert_eq!(style, "color: red; width: 11px;", "{operation}: {body}");
                let after = realm.engine.stats();
                after.validate_after(&before).unwrap();
                assert_eq!(after.source_admissions, before.source_admissions + 1);
                assert_eq!(after.source_bytes, before.source_bytes + source.len());
                let mut reply = BoaPageRealm::empty_reply();
                realm.snapshot(&mut reply);
                assert_eq!(reply.state, RealmState::Fatal);
                assert!(reply.task_wake.is_none());
                assert!(reply.navigation.is_none());
                assert_eq!(reply.default_action, DefaultAction::None);
                work_unchanged(&realm);
            }
        }
    }
}
