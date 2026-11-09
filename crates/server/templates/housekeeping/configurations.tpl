{% set title = locale.hk_configuracion_del_hotel %}
{% include "housekeeping/base/header.tpl" %}
{% autoescape 'html' %}
<form method="post" action="{{ hk.path }}/configurations" id="cfg-form">
  <input type="hidden" name="hk_token" value="{{ hk.token }}">

  <div class="toolbar">
    <input type="search" id="cfg-filter" placeholder="{{ locale.hk_buscar_ajuste_p_ej_credits_loader_poker|raw }}" value="{{ q }}">
    <span class="muted small">{{ total }} {{ locale.hk_ajustes_solo_se_guardan_los_que_cambies|raw }}</span>
    <span class="spacer"></span>
    <button class="btn primary" type="submit">{{ locale.hk_guardar_cambios|raw }}</button>
  </div>

  <div class="card mb cfg-group">
    <div class="card-head"><h2>{{ locale.hk_ajustes_principales|raw }}</h2></div>
    <div class="card-body scroll">
      <table class="tbl">
        <tbody>
        {% for c in important %}
          <tr class="cfg-row" data-key="{{ c.key }}">
            <td style="width:34%"><span class="mono">{{ c.key }}</span><div class="help">{{ c.help }}</div></td>
            <td>{% include "housekeeping/config_input.tpl" %}</td>
          </tr>
        {% endfor %}
        </tbody>
      </table>
    </div>
  </div>

  {% for group in groups %}
  <div class="card mb cfg-group">
    <div class="card-head"><h2 class="mono">{{ group.key }}.*</h2><span class="muted small">{{ group.value | length }}</span></div>
    <div class="card-body scroll">
      <table class="tbl">
        <tbody>
        {% for c in group.value %}
          <tr class="cfg-row" data-key="{{ c.key }}">
            <td style="width:34%"><span class="mono">{{ c.key }}</span></td>
            <td>{% if c.important %}<span class="muted small">{{ locale.hk_se_edita_arriba_en_ajustes_principales|raw }}</span>{% else %}{% include "housekeeping/config_input.tpl" %}{% endif %}</td>
          </tr>
        {% endfor %}
        </tbody>
      </table>
    </div>
  </div>
  {% endfor %}

  <div class="form-actions"><button class="btn primary" type="submit">{{ locale.hk_guardar_cambios|raw }}</button><span class="help">{{ locale.hk_al_guardar_se_recargan_los_ajustes_en_la_web_y_e|raw }}</span></div>
</form>
{% endautoescape %}
<script>
(function () {
  var input = document.getElementById('cfg-filter');
  function apply() {
    var q = input.value.toLowerCase().trim();
    document.querySelectorAll('.cfg-group').forEach(function (g) {
      var visible = 0;
      g.querySelectorAll('.cfg-row').forEach(function (r) {
        var show = !q || r.getAttribute('data-key').toLowerCase().indexOf(q) !== -1;
        r.style.display = show ? '' : 'none';
        if (show) visible++;
      });
      g.style.display = visible ? '' : 'none';
    });
  }
  input.addEventListener('input', apply);
  input.addEventListener('keydown', function (e) { if (e.key === 'Enter') e.preventDefault(); });
  apply();
})();
</script>
{% include "housekeeping/base/footer.tpl" %}
