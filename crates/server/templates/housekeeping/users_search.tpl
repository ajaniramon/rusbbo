{% set title = locale.hk_usuarios %}
{% include "housekeeping/base/header.tpl" %}
{% autoescape 'html' %}
<div class="toolbar">
  <form method="get" action="{{ hk.path }}/users">
    <input type="search" name="q" value="{{ q }}" placeholder="{{ locale.hk_nombre_email_mision_o_id|raw }}" autofocus>
    <select name="filter">
      <option value="" {% if filter == '' %}selected{% endif %}>{{ locale.hk_todos|raw }}</option>
      <option value="online" {% if filter == 'online' %}selected{% endif %}>{{ locale.hk_conectados|raw }}</option>
      <option value="staff" {% if filter == 'staff' %}selected{% endif %}>{{ locale.hk_staff_rango_2|raw }}</option>
      <option value="club" {% if filter == 'club' %}selected{% endif %}>{{ locale.hk_socios_hc|raw }}</option>
      <option value="banned" {% if filter == 'banned' %}selected{% endif %}>{{ locale.hk_baneados|raw }}</option>
      <option value="nocredits" {% if filter == 'nocredits' %}selected{% endif %}>{{ locale.hk_sin_creditos|raw }}</option>
    </select>
    <select name="sort">
      <option value="" {% if sort == '' %}selected{% endif %}>{{ locale.hk_mas_recientes_2|raw }}</option>
      <option value="last_online" {% if sort == 'last_online' %}selected{% endif %}>{{ locale.hk_ultima_conexion|raw }}</option>
      <option value="name" {% if sort == 'name' %}selected{% endif %}>{{ locale.hk_nombre|raw }}</option>
      <option value="credits" {% if sort == 'credits' %}selected{% endif %}>{{ locale.hk_creditos_2|raw }}</option>
      <option value="id" {% if sort == 'id' %}selected{% endif %}>{{ locale.hk_id|raw }}</option>
    </select>
    <button class="btn primary" type="submit">{{ locale.hk_buscar|raw }}</button>
    {% if q != '' or filter != '' or sort != '' %}<a class="btn ghost" href="{{ hk.path }}/users">{{ locale.hk_limpiar|raw }}</a>{% endif %}
  </form>
  <span class="spacer"></span>
  {% if hk.can('user/create') %}<a class="btn" href="{{ hk.path }}/users/create">{{ locale.hk_crear_usuario_2|raw }}</a>{% endif %}
</div>

<div class="card flush">
  <div class="card-body scroll">
    {% if players is empty %}
      <div class="empty">{{ locale.hk_no_hay_usuarios_que_coincidan|raw }}</div>
    {% else %}
    <table class="tbl">
      <thead>
        <tr><th>{{ locale.hk_usuario|raw }}</th><th>{{ locale.hk_estado|raw }}</th><th>{{ locale.hk_rango|raw }}</th><th class="num">{{ locale.hk_creditos_2|raw }}</th><th class="num">{{ locale.hk_pixeles_2|raw }}</th><th>{{ locale.hk_hc|raw }}</th><th>{{ locale.hk_ultima_conexion|raw }}</th><th>{{ locale.hk_registro|raw }}</th><th></th></tr>
      </thead>
      <tbody>
      {% for p in players %}
        <tr>
          <td>
            <div class="user-cell">
              <img class="avatar-head" src="/habbo-imaging/avatarimage?figure={{ p.figure }}&size=s&head=1&direction=3&head_direction=3" alt="">
              <div>
                <a href="{{ hk.path }}/users/view?id={{ p.id }}"><strong>{{ p.username }}</strong></a> <span class="muted small">#{{ p.id }}</span>
                <div class="muted small">{{ p.motto }}</div>
              </div>
            </div>
          </td>
          <td>
            {% if p.is_online %}<span class="pill ok"><span class="dot ok"></span> {{ locale.hk_conectado_2|raw }}</span>{% else %}<span class="pill">{{ locale.hk_desconectado|raw }}</span>{% endif %}
            {% if p.is_banned %}<span class="pill bad">{{ locale.hk_baneado|raw }}</span>{% endif %}
          </td>
          <td>{% if p.rank > 1 %}<span class="pill accent">{{ hk.rankLabel(p.rank) }}</span>{% else %}<span class="muted">{{ hk.rankLabel(p.rank) }}</span>{% endif %}</td>
          <td class="num">{{ p.credits }}</td>
          <td class="num">{{ p.pixels }}</td>
          <td>{% if p.is_club %}<span class="pill info" title="{{ locale.hk_hasta|raw }} {{ p.club_until }}">{{ locale.hk_hc|raw }}</span>{% else %}<span class="muted">&#8212;</span>{% endif %}</td>
          <td class="nowrap muted">{{ p.last_online }}</td>
          <td class="nowrap muted">{{ p.created_at }}</td>
          <td class="actions"><a class="btn sm" href="{{ hk.path }}/users/view?id={{ p.id }}">{{ locale.hk_abrir_ficha|raw }}</a></td>
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
