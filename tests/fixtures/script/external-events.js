// Authored local acceptance input, not an upstream conformance test.
if (externalOrder !== 'inline-before') throw new Error('Inline shared state missing');
externalOrder = 'external';
starts++;
(function () {
  var count = 0, clicked = false;
  var form = document.getElementById('search');
  var query = document.getElementById('query');
  var state = document.getElementById('state');
  document.getElementById('cancel').onclick = function (event) {
    event.preventDefault();
    if (query.value !== 'Rust & café') throw new Error('Typed value did not reach external realm');
    clicked = true;
    state.textContent = 'link canceled';
  };
  form.onsubmit = function (event) {
    count++;
    if (!clicked || starts !== 1) { event.preventDefault(); throw new Error('Retained external state failed'); }
    if (count === 1) {
      event.preventDefault();
      document.getElementById('parking').appendChild(query);
      form.appendChild(query);
      var proof = document.createElement('input');
      proof.id = 'proof'; proof.type = 'hidden'; proof.name = 'proof';
      proof.value = 'pending'; form.appendChild(proof);
      state.textContent = 'first submit canceled';
    } else if (count === 2 && query.value === 'Rust & café again') {
      document.getElementById('proof').value = 'retained-1-2';
      form.action = '/event-search';
    } else { event.preventDefault(); throw new Error('Unexpected external activation'); }
  };
})();
