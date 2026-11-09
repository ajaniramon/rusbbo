{% if category != null %}{% set title = locale.hk_editar_categoria ~ category.name() %}{% else %}{% set title = locale.hk_nueva_categoria_2 %}{% endif %}
{% include "housekeeping/base/header.tpl" %}
{% autoescape 'html' %}
<div class="page-head"><div></div><div class="actions"><a class="btn" href="{{ hk.path }}/room_categories">{{ locale.hk_volver_a_categorias|raw }}</a></div></div>
<div class="card">
  <div class="card-body">
    <form class="form" method="post" action="{{ hk.path }}/room_categories/edit{% if category != null %}?id={{ category.id() }}{% endif %}">
      <input type="hidden" name="hk_token" value="{{ hk.token }}">
      <div class="row">
        <div class="field" style="grid-column: span 2"><label>{{ locale.hk_nombre|raw }}</label><input type="text" name="name" required value="{% if category != null %}{{ category.name() }}{% endif %}"></div>
        <div class="field"><label>{{ locale.hk_carpeta_padre|raw }}</label>
          <select name="parent_id">
            <option value="0" {% if category == null or category.parentId() <= 0 %}selected{% endif %}>{{ locale.hk_raiz_2|raw }}</option>
            {% for p in categories %}{% if p.node() %}<option value="{{ p.id() }}" {% if category != null and category.parentId() == p.id() %}selected{% endif %}>{{ p.name() }} (#{{ p.id() }})</option>{% endif %}{% endfor %}
          </select>
        </div>
        <div class="field"><label>{{ locale.hk_orden|raw }}</label><input type="number" name="order_id" value="{% if category != null %}{{ category.orderId() }}{% else %}0{% endif %}"></div>
      </div>
      <div class="row r-2">
        <div class="field"><label>{{ locale.hk_rango_minimo_para_verla|raw }}</label>
          <select name="minrole_access">{% for r in ranks %}<option value="{{ r.getRankId() }}" {% if category != null and category.minRoleAccess() == r.getRankId() %}selected{% elseif category == null and r.getRankId() == 1 %}selected{% endif %}>{{ r.getRankId() }} · {{ hk.rankLabel(r.getRankId()) }}</option>{% endfor %}</select>
        </div>
        <div class="field"><label>{{ locale.hk_rango_minimo_para_poner_salas_aqui|raw }}</label>
          <select name="minrole_setflatcat">{% for r in ranks %}<option value="{{ r.getRankId() }}" {% if category != null and category.minRoleSetFlatCat() == r.getRankId() %}selected{% elseif category == null and r.getRankId() == 1 %}selected{% endif %}>{{ r.getRankId() }} · {{ hk.rankLabel(r.getRankId()) }}</option>{% endfor %}</select>
        </div>
      </div>
      <div class="form-actions">
        <label class="check"><input type="checkbox" name="isnode" {% if category != null and category.node() %}checked{% endif %}> {{ locale.hk_es_carpeta_contiene_otras_categorias|raw }}</label>
        <label class="check"><input type="checkbox" name="public_spaces" {% if category != null and category.publicSpaces() %}checked{% endif %}> {{ locale.hk_salas_publicas|raw }}</label>
        <label class="check"><input type="checkbox" name="allow_trading" {% if category == null or category.allowTrading() %}checked{% endif %}> {{ locale.hk_permite_tradeo|raw }}</label>
        <label class="check"><input type="checkbox" name="club_only" {% if category != null and category.clubOnly() %}checked{% endif %}> {{ locale.hk_solo_hc|raw }}</label>
        <label class="check"><input type="checkbox" name="is_top_priority" {% if category != null and category.topPriority() %}checked{% endif %}> {{ locale.hk_prioritaria|raw }}</label>
      </div>
      <div class="form-actions"><button class="btn primary" type="submit">{{ locale.hk_guardar_categoria|raw }}</button><span class="help">{{ locale.hk_el_navegador_del_juego_se_recarga_al_guardar|raw }}</span></div>
    </form>
  </div>
</div>
{% endautoescape %}
{% include "housekeeping/base/footer.tpl" %}
