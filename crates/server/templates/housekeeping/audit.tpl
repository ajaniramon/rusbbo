{% set title = locale.hk_registro_del_staff %}
{% include "housekeeping/base/header.tpl" %}
{% autoescape 'html' %}
<div class="toolbar">
  <form method="get" action="{{ hk.path }}/audit">
    <input type="search" name="q" value="{{ q }}" placeholder="{{ locale.hk_staff_usuario_afectado_o_accion|raw }}">
    <button class="btn primary" type="submit">{{ locale.hk_buscar|raw }}</button>
    {% if q != '' %}<a class="btn ghost" href="{{ hk.path }}/audit">{{ locale.hk_limpiar|raw }}</a>{% endif %}
  </form>
</div>
<div class="card flush">
  <div class="card-body scroll">
    {% if entries is empty %}<div class="empty">{{ locale.hk_no_hay_acciones_registradas|raw }}</div>{% else %}
    <table class="tbl">
      <thead><tr><th>{{ locale.hk_cuando|raw }}</th><th>{{ locale.hk_staff|raw }}</th><th>{{ locale.hk_accion|raw }}</th><th>{{ locale.hk_sobre|raw }}</th><th>{{ locale.hk_detalles|raw }}</th></tr></thead>
      <tbody>
      {% for a in entries %}
        <tr>
          <td class="nowrap muted">{{ a.created_at }}</td>
          <td>{% if a.staff_name != '' %}<strong>{{ a.staff_name }}</strong>{% else %}<span class="muted">{{ locale.hk_sistema|raw }}</span>{% endif %}</td>
          <td><span class="pill">{{ hk.actionLabel(a.action) }}</span></td>
          <td>
            {% if a.target_type == 'user' and a.target_id > 0 %}<a href="{{ hk.path }}/users/view?id={{ a.target_id }}">{{ a.target_name | default('#' ~ a.target_id) }}</a>
            {% elseif a.target_type == 'room' and a.target_id > 0 %}<a href="{{ hk.path }}/rooms/view?id={{ a.target_id }}">{{ locale.hk_sala|raw }} {{ a.target_name | default('#' ~ a.target_id) }}</a>
            {% elseif a.target_type == 'hotel' %}{{ locale.hk_todo_el_hotel|raw }}

            {% else %}{{ a.target_name }}{% endif %}
          </td>
          <td class="wrap">{{ a.details }}</td>
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
