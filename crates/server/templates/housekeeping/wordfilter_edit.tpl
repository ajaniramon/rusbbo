{% set title = (word != null) ? (locale.hk_editar_palabra ~ word.word()) : locale.hk_nueva_palabra_del_filtro %}
{% include "housekeeping/base/header.tpl" %}
{% autoescape 'html' %}
<div class="page-head">
  <div><p>{{ locale.hk_los_cambios_se_envian_al_juego_al_momento|raw }}</p></div>
  <div class="actions"><a class="btn" href="{{ hk.path }}/wordfilter">{{ locale.hk_volver_al_filtro|raw }}</a></div>
</div>
<div class="card" style="max-width:640px">
  <div class="card-body">
    <form class="form" method="post" action="{{ hk.path }}/wordfilter/edit{% if word != null %}?id={{ word.id() }}{% endif %}">
      <input type="hidden" name="hk_token" value="{{ hk.token }}">
      <div class="field"><label>{{ locale.hk_palabra|raw }}</label><input type="text" name="word" required maxlength="100" value="{% if word != null %}{{ word.word() }}{% endif %}"></div>
      <div class="form-actions">
        <label class="check"><input type="checkbox" name="is_filterable" {% if word == null or word.filterable() %}checked{% endif %}> {{ locale.hk_filtrar_sustituir_en_el_chat|raw }}</label>
        <label class="check"><input type="checkbox" name="is_bannable" {% if word != null and word.bannable() %}checked{% endif %}> {{ locale.hk_baneable|raw }}</label>
      </div>
      <div class="form-actions"><button class="btn primary" type="submit">{% if word != null %}{{ locale.hk_guardar_palabra|raw }}{% else %}{{ locale.hk_anadir_palabra|raw }}{% endif %}</button></div>
    </form>
  </div>
</div>
{% endautoescape %}
{% include "housekeeping/base/footer.tpl" %}
