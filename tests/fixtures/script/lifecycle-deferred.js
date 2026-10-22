// Authored same-origin deferred source: never a testharness/scoring input.
if (document.readyState !== 'interactive' || deferred) throw new Error('Wrong deferred readiness or duplicate source');
deferred = true;
lifecycleOrder.push('defer:' + document.readyState);
Promise.resolve().then(function () { lifecycleOrder.push('defer-job:' + document.readyState); });
