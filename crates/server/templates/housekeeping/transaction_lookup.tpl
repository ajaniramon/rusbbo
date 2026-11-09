{% set title = locale.hk_compras_y_transacciones %}
{% include "housekeeping/base/header.tpl" %}
{% autoescape 'html' %}
<div class="toolbar">
  <form method="get" action="{{ hk.path }}/transaction/lookup">
    <input type="text" name="user" value="{{ user }}" placeholder="{{ locale.hk_usuario_exacto|raw }}" style="width:170px">
    <input type="text" name="text" value="{{ text }}" placeholder="{{ locale.hk_descripcion_p_ej_monolith|raw }}">
    <input type="text" name="item" value="{{ item }}" placeholder="{{ locale.hk_id_de_furni|raw }}" style="width:120px">
    <select name="days">
      <option value="1" {% if days == 1 %}selected{% endif %}>{{ locale.hk_ultimo_dia|raw }}</option>
      <option value="7" {% if days == 7 %}selected{% endif %}>{{ locale.hk_ultimos_7_dias|raw }}</option>
      <option value="30" {% if days == 30 %}selected{% endif %}>{{ locale.hk_ultimos_30_dias|raw }}</option>
      <option value="0" {% if days == 0 %}selected{% endif %}>{{ locale.hk_todo|raw }}</option>
    </select>
    <button class="btn primary" type="submit">{{ locale.hk_filtrar_2|raw }}</button>
    {% if user != '' or text != '' or item != '' or days != 30 %}<a class="btn ghost" href="{{ hk.path }}/transaction/lookup">{{ locale.hk_limpiar|raw }}</a>{% endif %}
  </form>
</div>

<div class="stats mb">
  <div class="stat"><div class="label">{{ locale.hk_movimientos|raw }}</div><div class="value">{{ totals.cnt }}</div></div>
  <div class="stat"><div class="label">{{ locale.hk_creditos_gastados|raw }}</div><div class="value">{{ totals.credits }}</div></div>
  <div class="stat"><div class="label">{{ locale.hk_pixeles_gastados|raw }}</div><div class="value">{{ totals.pixels }}</div></div>
</div>

<div class="card flush">
  <div class="card-body scroll">
    {% if transactions is empty %}<div class="empty">{{ locale.hk_no_hay_movimientos_con_esos_filtros|raw }}</div>{% else %}
    <table class="tbl">
      <thead><tr><th>{{ locale.hk_cuando|raw }}</th><th>{{ locale.hk_usuario|raw }}</th><th>{{ locale.hk_descripcion|raw }}</th><th class="num">{{ locale.hk_cant|raw }}</th><th class="num">{{ locale.hk_creditos_2|raw }}</th><th class="num">{{ locale.hk_pixeles_2|raw }}</th><th>{{ locale.hk_furni_s|raw }}</th></tr></thead>
      <tbody>
      {% for t in transactions %}
        <tr>
          <td class="nowrap muted">{{ t.created_at }}</td>
          <td><a href="{{ hk.path }}/users/view?id={{ t.user_id }}&tab=transactions">{{ t.username }}</a></td>
          <td class="wrap">{{ t.description }}{% if not t.is_visible %} <span class="pill" title="{{ locale.hk_no_se_muestra_al_usuario|raw }}">{{ locale.hk_interna|raw }}</span>{% endif %}</td>
          <td class="num">{{ t.amount }}</td>
          <td class="num">{{ t.credit_cost }}</td>
          <td class="num">{{ t.pixel_cost }}</td>
          <td class="mono small wrap">{% if t.item_id != '' and t.item_id != '0' %}<a href="{{ hk.path }}/transaction/lookup?days=0&item={{ t.item_id }}">{{ t.item_id }}</a>{% else %}<span class="muted">&#8212;</span>{% endif %}</td>
        </tr>
      {% endfor %}
      </tbody>
    </table>
    {% endif %}
  </div>
  {% include "housekeeping/base/pager.tpl" %}
</div>
{% endautoescape %}
{% include "housekeeping/base/footer.tpl" %}
