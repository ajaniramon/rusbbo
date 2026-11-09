{% set title = locale.hk_salas %}
{% include "housekeeping/base/header.tpl" %}
{% autoescape 'html' %}
<div class="toolbar">
  <form method="get" action="{{ hk.path }}/rooms">
    <input type="search" name="query" value="{{ query }}" placeholder="{{ locale.hk_nombre_dueno_o_id_de_sala|raw }}">
    <select name="filter">
      <option value="" {% if filter == '' %}selected{% endif %}>{{ locale.hk_todas|raw }}</option>
      <option value="occupied" {% if filter == 'occupied' %}selected{% endif %}>{{ locale.hk_con_gente_dentro|raw }}</option>
      <option value="user" {% if filter == 'user' %}selected{% endif %}>{{ locale.hk_de_usuarios|raw }}</option>
      <option value="public" {% if filter == 'public' %}selected{% endif %}>{{ locale.hk_publicas_2|raw }}</option>
      <option value="hidden" {% if filter == 'hidden' %}selected{% endif %}>{{ locale.hk_ocultas|raw }}</option>
      <option value="picks" {% if filter == 'picks' %}selected{% endif %}>{{ locale.hk_recomendadas_del_staff|raw }}</option>
    </select>
    <button class="btn primary" type="submit">{{ locale.hk_buscar|raw }}</button>
    {% if query != '' or filter != '' %}<a class="btn ghost" href="{{ hk.path }}/rooms">{{ locale.hk_limpiar|raw }}</a>{% endif %}
  </form>
</div>

<div class="card flush">
  <div class="card-body scroll">
    {% if rooms is empty %}<div class="empty">{{ locale.hk_no_hay_salas_que_coincidan|raw }}</div>{% else %}
    <table class="tbl">
      <thead><tr><th>{{ locale.hk_sala|raw }}</th><th>{{ locale.hk_dueno|raw }}</th><th>{{ locale.hk_categoria|raw }}</th><th>{{ locale.hk_modelo|raw }}</th><th>{{ locale.hk_acceso|raw }}</th><th class="num">{{ locale.hk_furnis|raw }}</th><th class="num">{{ locale.hk_dentro_2|raw }}</th><th></th></tr></thead>
      <tbody>
      {% for r in rooms %}
        <tr>
          <td>
            <a href="{{ hk.path }}/rooms/view?id={{ r.id }}"><strong>{{ r.name }}</strong></a> <span class="muted small">#{{ r.id }}</span>
            {% if r.is_hidden %}<span class="pill warn">{{ locale.hk_oculta|raw }}</span>{% endif %}
            {% if r.is_staff_pick %}<span class="pill accent">{{ locale.hk_recomendada|raw }}</span>{% endif %}
          </td>
          <td>{% if r.owner_name != '' %}<a href="{{ hk.path }}/users/view?id={{ r.owner_id }}">{{ r.owner_name }}</a>{% else %}<span class="muted">{{ locale.hk_publica_2|raw }}</span>{% endif %}</td>
          <td class="muted">{{ r.category_name | default(r.category) }}</td>
          <td class="mono">{{ r.model }}</td>
          <td>{% if r.accesstype == 1 %}{{ locale.hk_timbre|raw }}{% elseif r.accesstype == 2 %}{{ locale.hk_contrasena|raw }}{% else %}{{ locale.hk_abierta|raw }}{% endif %}</td>
          <td class="num">{{ r.item_count }}</td>
          <td class="num">{% if r.visitors_now > 0 %}<span class="pill ok">{{ r.visitors_now }} / {{ r.visitors_max }}</span>{% else %}<span class="muted">0 / {{ r.visitors_max }}</span>{% endif %}</td>
          <td class="actions">
            <a class="btn sm" href="{{ hk.path }}/rooms/view?id={{ r.id }}">{{ locale.hk_ver|raw }}</a>
            {% if canManage %}
              <a class="btn sm" href="{{ hk.path }}/rooms/edit?id={{ r.id }}">{{ locale.hk_editar|raw }}</a>
              <form class="inline" method="post" action="{{ hk.path }}/rooms/staff_pick?id={{ r.id }}&enabled={% if r.is_staff_pick %}false{% else %}true{% endif %}">
                <input type="hidden" name="hk_token" value="{{ hk.token }}">
                <button class="btn sm" type="submit" title="{{ locale.hk_recomendada_por_el_staff_en_la_web|raw }}">{% if r.is_staff_pick %}{{ locale.hk_quitar_recomendacion|raw }}{% else %}{{ locale.hk_recomendar|raw }}{% endif %}</button>
              </form>
              <form class="inline" method="post" action="{{ hk.path }}/rooms/hide?id={{ r.id }}&hidden={% if r.is_hidden %}false{% else %}true{% endif %}">
                <input type="hidden" name="hk_token" value="{{ hk.token }}">
                <button class="btn sm" type="submit">{% if r.is_hidden %}{{ locale.hk_mostrar|raw }}{% else %}{{ locale.hk_ocultar|raw }}{% endif %}</button>
              </form>
            {% endif %}
          </td>
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
