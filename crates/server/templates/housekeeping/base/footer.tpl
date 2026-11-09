{% if hk.loggedIn %}
    </div>
    <div class="footer-note">{{ site.siteName }} {{ locale.hk_housekeeping_havana_los_cambios_hechos_aqui_qued|raw }}</div>
  </div>
</div>
{% endif %}
<script>
(function () {
  // Confirmation for dangerous actions: <form data-confirm="...">
  document.addEventListener('submit', function (e) {
    var form = e.target;
    var msg = form.getAttribute && form.getAttribute('data-confirm');
    if (msg && !window.confirm(msg)) { e.preventDefault(); }
  }, true);

  // Instant client-side filter for tables: <input data-filter="tableId">
  document.querySelectorAll('input[data-filter]').forEach(function (input) {
    var table = document.getElementById(input.getAttribute('data-filter'));
    if (!table) return;
    input.addEventListener('input', function () {
      var q = input.value.toLowerCase();
      table.querySelectorAll('tbody tr').forEach(function (tr) {
        tr.style.display = tr.textContent.toLowerCase().indexOf(q) === -1 ? 'none' : '';
      });
    });
  });

  // Client-side tabs: <div class="tabs"><a data-tab="x"> + <div class="tab-panel" data-panel="x">
  var tabs = document.querySelectorAll('.tabs a[data-tab]');
  function show(name) {
    var found = false;
    tabs.forEach(function (a) { var on = a.getAttribute('data-tab') === name; a.classList.toggle('on', on); found = found || on; });
    document.querySelectorAll('.tab-panel').forEach(function (p) { p.classList.toggle('on', p.getAttribute('data-panel') === name); });
    return found;
  }
  if (tabs.length) {
    tabs.forEach(function (a) {
      a.addEventListener('click', function (e) {
        e.preventDefault();
        var name = a.getAttribute('data-tab');
        show(name);
        if (history.replaceState) {
          var url = new URL(window.location.href);
          url.searchParams.set('tab', name);
          history.replaceState(null, '', url.toString());
        }
      });
    });
    var initial = new URLSearchParams(window.location.search).get('tab') || tabs[0].getAttribute('data-tab');
    if (!show(initial)) show(tabs[0].getAttribute('data-tab'));
  }
})();
</script>
</body>
</html>
