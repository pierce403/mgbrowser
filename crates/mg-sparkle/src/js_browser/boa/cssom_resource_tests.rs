// Frozen pre-implementation controls, included inside boa.rs's private tests.
// Uses only existing realm/evaluate/message_title and private host.allocated
// seams. No production hook, parser counter, profile or test-only GC repair.
#[cfg(test)]
mod cssom_resource_controls {
    use super::*;

    fn saved_style() -> BoaPageRealm {
        // Existing APIs only. Raw setAttribute deliberately does not pre-parse
        // or prewarm CSSOM. Capturing a method must not serialize a CSS value.
        realm(
            r#"
            var cssomNode=document.getElementById('out');
            cssomNode.setAttribute('style','color:red; width:7px');
            var cssomStyle=cssomNode.style;
            var cssomGet=cssomStyle.getPropertyValue;
            var cssomCaught=0, cssomContinued=false, cssomTouched=0;
            "#,
        )
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

    fn boolean(realm: &mut BoaPageRealm, name: &str) -> bool {
        realm.engine.get_global(name).unwrap().as_boolean().unwrap()
    }

    fn synchronous_work_only(realm: &BoaPageRealm) {
        let report = realm.engine.stats();
        // These are the two actual mandatory lifecycle tasks, not discounted
        // counters or an independent CSS work allowance.
        assert_eq!(
            (
                report.jobs_admitted,
                report.jobs_executed,
                report.pending_jobs
            ),
            (2, 2, 0)
        );
        assert_eq!(realm.lifecycle.phase, LifecyclePhase::LoadComplete);
        let data = realm.state.borrow();
        assert!(data.timers.is_empty());
        assert!(data.dom_tasks.is_empty());
        assert!(data.rejections.is_empty());
    }

    #[test]
    fn cssom_has_is_structural_without_value_getter_or_result_admission() {
        let mut realm = saved_style();
        // Existing attribute API admits this exact raw length. Its canonical
        // cssText would expand beyond the unchanged attribute ceiling, so `in`
        // must not invoke that getter as a presence test.
        evaluate(
            &mut realm,
            "cssomNode.setAttribute('style','--x:'+'a'.repeat(16380));",
        );
        let before = attributes(&realm);
        let methods = realm.state.borrow().methods.len();
        let wrappers = realm.state.borrow().wrappers_created;
        realm.state.borrow_mut().host.allocated = super::super::super::MAX_DOM_BYTES;
        evaluate(
            &mut realm,
            r#"
            for(var i=0;i<16;i++) {
                if(!Reflect.has(cssomStyle,'cssText')) throw 'missing cssText';
                if(!Reflect.has(cssomStyle,'getPropertyValue')) throw 'missing method';
                if(!Reflect.has(cssomStyle,'color')) throw 'missing enabled accessor';
                if(Reflect.has(cssomStyle,'notAnEnabledCssProperty')) throw 'invented accessor';
                if(Reflect.has(cssomStyle,'--privateCustomName')) throw 'custom named accessor';
            }
            cssomContinued=true;
            "#,
        );
        assert!(boolean(&mut realm, "cssomContinued"));
        assert_eq!(
            realm.state.borrow().host.allocated,
            super::super::super::MAX_DOM_BYTES
        );
        assert_eq!(realm.state.borrow().methods.len(), methods);
        assert_eq!(realm.state.borrow().wrappers_created, wrappers);
        assert_eq!(attributes(&realm), before);
        assert!(!realm.engine.is_fatal());
        synchronous_work_only(&realm);
        // No extra parser-observer counter is introduced. The no-parser part
        // additionally requires the reviewed structural `has` source path.
    }

    #[test]
    fn cssom_actual_get_and_set_traps_reject_wrong_receiver_before_conversion() {
        let mut realm = saved_style();
        let before = attributes(&realm);
        let admitted = realm.state.borrow().host.allocated;
        evaluate(
            &mut realm,
            r#"
            var value={toString(){cssomTouched++;return 'blue';}};
            var wrong={toString(){cssomTouched++;return 'receiver';}};
            try { Reflect.get(cssomStyle,'cssText',wrong); }
            catch(error) { if(!(error instanceof TypeError)) throw error; cssomCaught++; }
            try { Reflect.set(cssomStyle,'color',value,wrong); }
            catch(error) { if(!(error instanceof TypeError)) throw error; cssomCaught++; }
            cssomContinued=true;
            "#,
        );
        assert_eq!(number(&mut realm, "cssomCaught"), 2.0);
        assert_eq!(number(&mut realm, "cssomTouched"), 0.0);
        assert!(boolean(&mut realm, "cssomContinued"));
        assert_eq!(realm.state.borrow().host.allocated, admitted);
        assert_eq!(attributes(&realm), before);
        assert!(!realm.engine.is_fatal());
        synchronous_work_only(&realm);
    }

    #[test]
    fn cssom_method_receiver_rejection_does_not_coerce_its_name_argument() {
        let mut realm = saved_style();
        evaluate(
            &mut realm,
            "if(typeof cssomGet!=='function') throw 'missing CSSOM method';",
        );
        let before = attributes(&realm);
        evaluate(
            &mut realm,
            r#"
            var name={toString(){cssomTouched++;return 'color';}};
            try { cssomGet.call({},name); }
            catch(error) { if(!(error instanceof TypeError)) throw error; cssomCaught++; }
            cssomContinued=true;
            "#,
        );
        assert_eq!(number(&mut realm, "cssomCaught"), 1.0);
        assert_eq!(number(&mut realm, "cssomTouched"), 0.0);
        assert_eq!(attributes(&realm), before);
        assert!(!realm.engine.is_fatal());
        synchronous_work_only(&realm);
    }

    #[test]
    fn cssom_getter_host_quota_error_is_ordinary_and_not_empty_success() {
        for expression in ["cssomGet.call(cssomStyle,'color')", "cssomStyle.cssText"] {
            let mut realm = saved_style();
            evaluate(
                &mut realm,
                "if(typeof cssomGet!=='function') throw 'missing CSSOM method';",
            );
            let before = attributes(&realm);
            realm.state.borrow_mut().host.allocated = super::super::super::MAX_DOM_BYTES;
            evaluate(
                &mut realm,
                &format!(
                    "try {{ {expression}; }} catch(error) {{ if(!(error instanceof TypeError)) throw error; cssomCaught++; }} cssomContinued=true;"
                ),
            );
            assert_eq!(number(&mut realm, "cssomCaught"), 1.0, "{expression}");
            assert!(boolean(&mut realm, "cssomContinued"));
            assert!(
                realm.state.borrow().host.allocated > super::super::super::MAX_DOM_BYTES,
                "failed actual result admission must not be free/refunded"
            );
            assert_eq!(attributes(&realm), before);
            assert!(
                !realm.engine.is_fatal(),
                "ordinary Host admission was relabeled fatal"
            );
            synchronous_work_only(&realm);
        }
    }

    #[test]
    fn cssom_commit_host_quota_error_leaves_the_whole_prior_attribute_untouched() {
        for assignment in [
            "cssomStyle.color='blue'",
            "cssomStyle.cssText='width:8px;color:blue'",
        ] {
            let mut realm = saved_style();
            let before = attributes(&realm);
            realm.state.borrow_mut().host.allocated = super::super::super::MAX_DOM_BYTES;
            evaluate(
                &mut realm,
                &format!(
                    "try {{ {assignment}; }} catch(error) {{ if(!(error instanceof TypeError)) throw error; cssomCaught++; }} cssomContinued=true;"
                ),
            );
            assert_eq!(number(&mut realm, "cssomCaught"), 1.0, "{assignment}");
            assert!(boolean(&mut realm, "cssomContinued"));
            assert!(realm.state.borrow().host.allocated > super::super::super::MAX_DOM_BYTES);
            assert_eq!(
                attributes(&realm),
                before,
                "no partial replacement, removal or new slot"
            );
            assert!(!realm.engine.is_fatal());
            synchronous_work_only(&realm);
        }
    }

    #[test]
    fn cssom_oversize_and_canonical_expansion_are_errors_without_mutation() {
        let mut realm = saved_style();
        evaluate(
            &mut realm,
            "if(typeof cssomGet!=='function') throw 'missing CSSOM method';",
        );
        let before = attributes(&realm);
        evaluate(
            &mut realm,
            r#"
            var tooLarge='x'.repeat(16385);
            var exactInput='--x:'+'a'.repeat(16380);
            if(exactInput.length!==16384) throw 'wrong authored boundary';
            try { cssomStyle.color=tooLarge; } catch(error) { if(!(error instanceof TypeError)) throw error; cssomCaught++; }
            try { cssomStyle.cssText=tooLarge; } catch(error) { if(!(error instanceof TypeError)) throw error; cssomCaught++; }
            try { cssomGet.call(cssomStyle,tooLarge); } catch(error) { if(!(error instanceof TypeError)) throw error; cssomCaught++; }
            try { cssomStyle.cssText=exactInput; } catch(error) { if(!(error instanceof TypeError)) throw error; cssomCaught++; }
            cssomContinued=true;
            "#,
        );
        assert_eq!(number(&mut realm, "cssomCaught"), 4.0);
        assert!(boolean(&mut realm, "cssomContinued"));
        assert_eq!(attributes(&realm), before);
        assert!(!realm.engine.is_fatal());
        synchronous_work_only(&realm);
    }

    #[test]
    fn cssom_conversion_keeps_ordinary_thrown_identity_and_has_no_outer_mutation() {
        let mut realm = saved_style();
        evaluate(
            &mut realm,
            "if(typeof cssomGet!=='function') throw 'missing CSSOM method';",
        );
        let before = attributes(&realm);
        evaluate(
            &mut realm,
            r#"
            var original={};
            var thrown={toString(){cssomTouched++;throw original;}};
            try { cssomStyle.color=thrown; } catch(error) { if(error!==original) throw 'lost setter exception'; cssomCaught++; }
            try { cssomGet.call(cssomStyle,thrown); } catch(error) { if(error!==original) throw 'lost method exception'; cssomCaught++; }
            cssomContinued=true;
            "#,
        );
        assert_eq!(number(&mut realm, "cssomCaught"), 2.0);
        assert_eq!(number(&mut realm, "cssomTouched"), 2.0);
        assert!(boolean(&mut realm, "cssomContinued"));
        assert_eq!(attributes(&realm), before);
        assert!(!realm.engine.is_fatal());
        evaluate(
            &mut realm,
            "cssomStyle.width='8px';if(cssomGet.call(cssomStyle,'width')!=='8px') throw 'ordinary throw poisoned realm';",
        );
        synchronous_work_only(&realm);
    }

    #[test]
    fn cssom_conversion_reentry_reads_current_dom_without_a_live_borrow() {
        let mut realm = saved_style();
        evaluate(
            &mut realm,
            r#"
            cssomStyle.color={toString(){cssomTouched++;cssomStyle.width='11px';return 'blue';}};
            if(cssomStyle.getPropertyValue('width')!=='11px') throw 'stale outer attribute';
            if(cssomStyle.getPropertyValue('color')!=='blue') throw 'outer assignment lost';
            cssomContinued=true;
            "#,
        );
        assert_eq!(number(&mut realm, "cssomTouched"), 1.0);
        assert!(boolean(&mut realm, "cssomContinued"));
        assert!(!realm.engine.is_fatal());
        synchronous_work_only(&realm);
    }

    #[test]
    fn cssom_conversion_preserves_existing_recursion_and_opcode_fatal_without_outer_mutation() {
        for conversion in [
            "function recurse(){return recurse();}return recurse();",
            "while(true){}",
        ] {
            let mut realm = saved_style();
            let before = attributes(&realm);
            let result = realm.engine.evaluate(&format!(
                "var value={{toString(){{{conversion}}}}};try{{cssomStyle.color=value;}}catch(error){{document.title='caught fatal';}}document.title='continued after fatal';"
            ));
            assert!(result.is_err(), "{conversion}");
            assert!(realm.engine.is_fatal());
            assert_eq!(attributes(&realm), before);
            assert_eq!(message_title(&realm), "");
            let mut reply = BoaPageRealm::empty_reply();
            realm.snapshot(&mut reply);
            assert_eq!(reply.state, RealmState::Fatal);
            assert!(reply.navigation.is_none());
            assert!(reply.task_wake.is_none());
            assert_eq!(reply.default_action, DefaultAction::None);
            synchronous_work_only(&realm);
        }
    }
}
