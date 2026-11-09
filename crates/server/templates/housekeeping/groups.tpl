{% set title = locale.hk_grupos %}
{% include "housekeeping/base/header.tpl" %}
{% autoescape 'html' %}
<div class="toolbar">
  <form method="get" action="{{ hk.path }}/groups">
    <input type="search" name="query" value="{{ query }}" placeholder="{{ locale.hk_id_nombre_alias_o_dueno|raw }}">
    <button class="btn primary" type="submit">{{ locale.hk_buscar|raw }}</button>
    {% if query != '' %}<a class="btn ghost" href="{{ hk.path }}/groups">{{ locale.hk_limpiar|raw }}</a>{% endif %}
  </form>
</div>
<div class="card flush">
  <div class="card-body scroll">
    {% if groups is empty %}
      <div class="empty">{% if query != '' %}{{ locale.hk_ningun_grupo_coincide|raw }}{% else %}{{ locale.hk_todavia_no_hay_grupos_en_el_hotel_los_usuarios_l|raw }}{% endif %}</div>
    {% else %}
    <table class="tbl">
      <thead><tr><th>{{ locale.hk_grupo_3|raw }}</th><th>{{ locale.hk_dueno|raw }}</th><th>{{ locale.hk_sala|raw }}</th><th class="num">{{ locale.hk_miembros_2|raw }}</th><th class="num">{{ locale.hk_pendientes|raw }}</th><th class="num">{{ locale.hk_temas_2|raw }}</th><th></th></tr></thead>
      <tbody>
      {% for g in groups %}
        <tr>
          <td><strong>{{ g.name() }}</strong> <span class="muted small">#{{ g.id() }}{% if g.alias() != '' %} · {{ g.alias() }}{% endif %}</span>{% if g.staffPick() %} <span class="pill accent">{{ locale.hk_recomendado|raw }}</span>{% endif %}</td>
          <td><a href="{{ hk.path }}/users/view?id={{ g.ownerId() }}">{{ g.ownerName() }}</a></td>
          <td>{% if g.roomId() > 0 %}<a href="{{ hk.path }}/rooms/view?id={{ g.roomId() }}">#{{ g.roomId() }}</a>{% else %}<span class="muted">&#8212;</span>{% endif %}</td>
          <td class="num">{{ g.memberCount() }}</td>
          <td class="num">{{ g.pendingCount() }}</td>
          <td class="num">{{ g.threadCount() }}</td>
          <td class="actions">
            <a class="btn sm" href="{{ hk.path }}/groups/edit?id={{ g.id() }}">{{ locale.hk_editar|raw }}</a>
            <form class="inline" method="post" action="{{ hk.path }}/groups/staff_pick?id={{ g.id() }}&enabled={% if g.staffPick() %}false{% else %}true{% endif %}">
              <input type="hidden" name="hk_token" value="{{ hk.token }}"><button class="btn sm" type="submit">{% if g.staffPick() %}{{ locale.hk_quitar_recomendacion|raw }}{% else %}{{ locale.hk_recomendar|raw }}{% endif %}</button>
            </form>
            <form class="inline" method="post" action="{{ hk.path }}/groups/delete?id={{ g.id() }}" data-confirm="{{ locale.hk_borrar_el_grupo|raw }}{{ g.name() }}{{ locale.hk_con_sus_miembros_y_foro|raw }}">
              <input type="hidden" name="hk_token" value="{{ hk.token }}"><button class="btn sm ghost-danger" type="submit">{{ locale.hk_borrar|raw }}</button>
            </form>
          </td>
        </tr>
      {% endfor %}
      </tbody>
    </table>
    {% endif %}
  </div>
</div>
{% endautoescape %}
{% include "housekeeping/base/footer.tpl" %}
