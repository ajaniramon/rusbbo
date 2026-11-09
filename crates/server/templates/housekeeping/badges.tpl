{% set title = locale.hk_placas %}
{% include "housekeeping/base/header.tpl" %}
{% autoescape 'html' %}
<div class="grid g-side">
  <div class="card">
    <div class="card-head"><h2>{{ locale.hk_buscar_asignaciones|raw }}</h2></div>
    <div class="card-body">
      <form class="toolbar" style="margin:0" method="get" action="{{ hk.path }}/badges">
        <input type="search" name="query" value="{{ query }}" placeholder="{{ locale.hk_id_de_usuario_nombre_o_codigo_de_placa|raw }}">
        <button class="btn primary" type="submit">{{ locale.hk_buscar|raw }}</button>
        {% if query != '' %}<a class="btn ghost" href="{{ hk.path }}/badges">{{ locale.hk_limpiar|raw }}</a>{% endif %}
      </form>
    </div>
  </div>
  <div class="card">
    <div class="card-head"><h2>{{ locale.hk_dar_una_placa|raw }}</h2></div>
    <div class="card-body">
      <form class="form" method="post" action="{{ hk.path }}/badges/grant">
        <input type="hidden" name="hk_token" value="{{ hk.token }}">
        <div class="row r-2">
          <div class="field"><label>{{ locale.hk_id_de_usuario|raw }}</label><input type="number" name="user_id" min="1" required></div>
          <div class="field"><label>{{ locale.hk_codigo_de_placa|raw }}</label><input type="text" name="badge" class="mono" required maxlength="50"></div>
        </div>
        <div class="form-actions"><button class="btn primary" type="submit">{{ locale.hk_dar_placa|raw }}</button><span class="help">{{ locale.hk_tambien_desde_la_ficha_del_usuario_placas_se_ve|raw }}</span></div>
      </form>
    </div>
  </div>
</div>

<div class="card flush mt">
  <div class="card-head"><h2>{{ locale.hk_asignaciones|raw }} <span class="sub">{{ assignments | length }}{% if query == '' %} {{ locale.hk_mas_recientes|raw }}{% endif %}</span></h2></div>
  <div class="card-body scroll">
    {% if assignments is empty %}<div class="empty">{{ locale.hk_no_hay_placas_que_coincidan|raw }}</div>{% else %}
    <table class="tbl">
      <thead><tr><th>{{ locale.hk_usuario|raw }}</th><th>{{ locale.hk_placa|raw }}</th><th>{{ locale.hk_puesta|raw }}</th><th>{{ locale.hk_ranura|raw }}</th><th></th></tr></thead>
      <tbody>
      {% for a in assignments %}
        <tr>
          <td><a href="{{ hk.path }}/users/view?id={{ a.userId() }}&tab=badges">{{ a.username() }}</a> <span class="muted small">#{{ a.userId() }}</span></td>
          <td><div class="user-cell"><img class="badge-img" src="/c_images/album1584/{{ a.badge() }}.gif" alt="" onerror="this.style.visibility='hidden'"><span class="mono">{{ a.badge() }}</span></div></td>
          <td><input type="checkbox" name="equipped" form="bu-{{ loop.index }}" {% if a.equipped() %}checked{% endif %}></td>
          <td><input type="number" name="slot_id" form="bu-{{ loop.index }}" min="0" max="5" value="{{ a.slotId() }}" style="width:80px"></td>
          <td class="actions">
            <form class="inline" id="bu-{{ loop.index }}" method="post" action="{{ hk.path }}/badges/update">
              <input type="hidden" name="hk_token" value="{{ hk.token }}">
              <input type="hidden" name="user_id" value="{{ a.userId() }}">
              <input type="hidden" name="badge" value="{{ a.badge() }}">
              <button class="btn sm" type="submit">{{ locale.hk_guardar|raw }}</button>
            </form>
            <form class="inline" method="post" action="{{ hk.path }}/badges/remove?user_id={{ a.userId() }}&badge={{ a.badge() }}" data-confirm="{{ locale.hk_quitar_la_placa|raw }} {{ a.badge() }} {{ locale.hk_a|raw }} {{ a.username() }}?">
              <input type="hidden" name="hk_token" value="{{ hk.token }}">
              <button class="btn sm ghost-danger" type="submit">{{ locale.hk_quitar|raw }}</button>
            </form>
          </td>
        </tr>
      {% endfor %}
      </tbody>
    </table>
    {% endif %}
  </div>
</div>

<div class="grid g-2 mt">
  <div class="card flush">
    <div class="card-head"><h2>{{ locale.hk_placas_del_hotel|raw }} <span class="sub">{{ catalogue | length }}</span></h2><input type="search" data-filter="badge-catalogue" placeholder="{{ locale.hk_filtrar|raw }}" style="width:160px"></div>
    <div class="card-body scroll scroll-y">
      {% if catalogue is empty %}<div class="empty">{{ locale.hk_ninguna_placa_asignada_todavia|raw }}</div>{% else %}
      <table class="tbl" id="badge-catalogue">
        <thead><tr><th>{{ locale.hk_placa|raw }}</th><th class="num">{{ locale.hk_usuarios|raw }}</th><th>{{ locale.hk_tipo|raw }}</th></tr></thead>
        <tbody>
        {% for b in catalogue %}
          <tr>
            <td><div class="user-cell"><img class="badge-img" src="/c_images/album1584/{{ b.badge() }}.gif" alt="" onerror="this.style.visibility='hidden'"><a class="mono" href="{{ hk.path }}/badges?query={{ b.badge() }}">{{ b.badge() }}</a></div></td>
            <td class="num">{{ b.assignmentCount() }}</td>
            <td>{% if b.rankBadge() %}<span class="pill accent">{{ locale.hk_de_rango|raw }}</span>{% else %}<span class="muted">{{ locale.hk_normal|raw }}</span>{% endif %}</td>
          </tr>
        {% endfor %}
        </tbody>
      </table>
      {% endif %}
    </div>
  </div>
  <div class="card flush">
    <div class="card-head"><h2>{{ locale.hk_ultimos_cambios_de_placas|raw }}</h2></div>
    <div class="card-body scroll scroll-y">
      {% if audit is empty %}<div class="empty">{{ locale.hk_sin_cambios_registrados|raw }}</div>{% else %}
      <table class="tbl">
        <thead><tr><th>{{ locale.hk_cuando|raw }}</th><th>{{ locale.hk_accion|raw }}</th><th>{{ locale.hk_staff|raw }}</th><th>{{ locale.hk_usuario|raw }}</th><th>{{ locale.hk_placa|raw }}</th></tr></thead>
        <tbody>
        {% for log in audit %}
          <tr>
            <td class="nowrap muted">{{ log.createdAt() }}</td>
            <td><span class="pill">{{ hk.actionLabel(log.action()) }}</span></td>
            <td><a href="{{ hk.path }}/users/view?id={{ log.staffId() }}">#{{ log.staffId() }}</a></td>
            <td><a href="{{ hk.path }}/users/view?id={{ log.targetId() }}&tab=badges">#{{ log.targetId() }}</a></td>
            <td class="mono">{{ log.message() }}</td>
          </tr>
        {% endfor %}
        </tbody>
      </table>
      {% endif %}
    </div>
  </div>
</div>
{% endautoescape %}
{% include "housekeeping/base/footer.tpl" %}
