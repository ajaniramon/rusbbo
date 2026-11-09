{% set title = locale.hk_usuario_2 ~ user.username %}
{% include "housekeeping/base/header.tpl" %}
{% autoescape 'html' %}
{% set back = hk.path ~ '/users/view?id=' ~ user.id %}

<div class="card mb">
  <div class="card-body">
    <div class="profile-head">
      <img class="avatar-full" src="/habbo-imaging/avatarimage?figure={{ user.figure }}&size=b&direction=2&head_direction=3" alt="" width="64" height="110">
      <div class="who" style="flex:1; min-width:240px">
        <h2>{{ user.username }} <span class="muted small">#{{ user.id }}</span></h2>
        <div class="motto">{% if user.motto != '' %}�{{ user.motto }}�{% else %}<span class="muted">{{ locale.hk_sin_mision|raw }}</span>{% endif %}</div>
        <div class="pills">
          {% if user.is_online %}<span class="pill ok"><span class="dot ok"></span> {{ locale.hk_conectado_2|raw }}{% if currentRoom != null %} � {{ currentRoom.name }}{% endif %}</span>{% else %}<span class="pill">{{ locale.hk_desconectado_visto|raw }} {{ user.last_online }}</span>{% endif %}
          <span class="pill {% if user.rank > 1 %}accent{% endif %}">{{ hk.rankLabel(user.rank) }} {{ locale.hk_rango_2|raw }} {{ user.rank }})</span>
          {% if user.is_club %}<span class="pill info">{{ locale.hk_hc_hasta|raw }} {{ user.club_until }}</span>{% endif %}
          {% for b in bans %}{% if b.is_active %}<span class="pill bad">{{ locale.hk_baneado_hasta|raw }} {{ b.banned_until }}</span>{% endif %}{% endfor %}
        </div>
      </div>
      <div class="stats profile-stats">
        <div class="stat"><div class="label">{{ locale.hk_creditos_2|raw }}</div><div class="value">{{ user.credits }}</div></div>
        <div class="stat"><div class="label">{{ locale.hk_pixeles_2|raw }}</div><div class="value">{{ user.pixels }}</div></div>
        <div class="stat"><div class="label">{{ locale.hk_tickets_2|raw }}</div><div class="value">{{ user.tickets }}</div></div>
        <div class="stat"><div class="label">{{ locale.hk_carretes|raw }}</div><div class="value">{{ user.film }}</div></div>
      </div>
    </div>
  </div>
</div>

<div class="tabs">
  <a href="?id={{ user.id }}&tab=summary" data-tab="summary">{{ locale.hk_resumen|raw }}</a>
  {% if canEdit %}<a href="?id={{ user.id }}&tab=edit" data-tab="edit">{{ locale.hk_editar|raw }}</a>{% endif %}
  {% if canManage and (canEdit or hk.can('bans')) %}<a href="?id={{ user.id }}&tab=moderation" data-tab="moderation">{{ locale.hk_moderacion|raw }}</a>{% endif %}
  <a href="?id={{ user.id }}&tab=badges" data-tab="badges">{{ locale.hk_placas|raw }} <span class="count">{{ badges | length }}</span></a>
  <a href="?id={{ user.id }}&tab=rooms" data-tab="rooms">{{ locale.hk_salas|raw }} <span class="count">{{ rooms | length }}</span></a>
  <a href="?id={{ user.id }}&tab=inventory" data-tab="inventory">{{ locale.hk_inventario|raw }} <span class="count">{{ inventoryTotals.in_hand | default(0) }}</span></a>
  <a href="?id={{ user.id }}&tab=friends" data-tab="friends">{{ locale.hk_amigos|raw }} <span class="count">{{ friends | length }}</span></a>
  <a href="?id={{ user.id }}&tab=chat" data-tab="chat">{{ locale.hk_chat|raw }}</a>
  <a href="?id={{ user.id }}&tab=transactions" data-tab="transactions">{{ locale.hk_compras|raw }}</a>
  <a href="?id={{ user.id }}&tab=access" data-tab="access">{{ locale.hk_accesos_e_ips|raw }}</a>
  <a href="?id={{ user.id }}&tab=history" data-tab="history">{{ locale.hk_historial_staff|raw }} <span class="count">{{ audit | length }}</span></a>
</div>

{# ------------------------------------------------------------------ summary #}
<div class="tab-panel" data-panel="summary">
  <div class="grid g-2">
    <div class="card">
      <div class="card-head"><h2>{{ locale.hk_datos_de_la_cuenta|raw }}</h2></div>
      <div class="card-body">
        <dl class="kv">
          <dt>{{ locale.hk_email|raw }}</dt><dd>{% if user.email != '' %}{{ user.email }}{% else %}<span class="muted">&#8212;</span>{% endif %}</dd>
          <dt>{{ locale.hk_registro|raw }}</dt><dd>{{ user.created_at }}</dd>
          <dt>{{ locale.hk_ultima_conexion|raw }}</dt><dd>{{ user.last_online }}</dd>
          <dt>{{ locale.hk_sexo|raw }}</dt><dd>{% if user.sex == 'F' %}{{ locale.hk_chica|raw }}{% else %}{{ locale.hk_chico|raw }}{% endif %}</dd>
          <dt>{{ locale.hk_look|raw }}</dt><dd class="mono">{{ user.figure }}</dd>
          <dt>{{ locale.hk_club_hc|raw }}</dt><dd>{% if user.is_club %}{{ locale.hk_activo_hasta|raw }} {{ user.club_until }}{% elseif user.club_until != '' %}{{ locale.hk_caducado|raw }}{{ user.club_until }}){% else %}{{ locale.hk_nunca|raw }}{% endif %}</dd>
          <dt>{{ locale.hk_respetos|raw }}</dt><dd>{{ user.respect_points }}</dd>
          <dt>{{ locale.hk_tradeo|raw }}</dt><dd>{% if user.is_trade_enabled %}{{ locale.hk_activado_2|raw }}{% else %}{{ locale.hk_desactivado_2|raw }}{% endif %}</dd>
          {% if stats != null %}
          <dt>{{ locale.hk_tiempo_conectado|raw }}</dt><dd>{{ (stats.online_time / 3600) | numberformat('#,##0.0') }} {{ locale.hk_h|raw }}</dd>
          <dt>{{ locale.hk_dias_seguidos|raw }}</dt><dd>{{ stats.days_logged_in_row }}</dd>
          {% endif %}
          <dt>{{ locale.hk_etiquetas|raw }}</dt><dd>{% for t in tags %}<span class="pill">{{ t.tag }}</span> {% else %}<span class="muted">&#8212;</span>{% endfor %}</dd>
        </dl>
      </div>
    </div>
    <div class="card flush">
      <div class="card-head"><h2>{{ locale.hk_ultimas_salas_visitadas|raw }}</h2></div>
      <div class="card-body">
        <ul class="list">
        {% for v in lastVisits %}
          <li><a href="{{ hk.path }}/rooms/view?id={{ v.id }}">{{ v.name }}</a><span class="muted small">{{ v.visited_at }}</span></li>
        {% else %}
          <li class="muted">{{ locale.hk_todavia_no_ha_entrado_en_ninguna_sala|raw }}</li>
        {% endfor %}
        </ul>
      </div>
    </div>
  </div>
  {% if hk.can('user/imitate') and canManage %}
  <div class="card mt">
    <div class="card-head"><h2>{{ locale.hk_herramientas|raw }}</h2></div>
    <div class="card-body form-actions">
      <form class="inline" method="post" action="{{ hk.path }}/users/imitate/{{ user.username }}" data-confirm="{{ locale.hk_vas_a_salir_del_panel_y_entrar_en_la_web_como|raw }} {{ user.username }}{{ locale.hk_seguro|raw }}">
        <input type="hidden" name="hk_token" value="{{ hk.token }}">
        <button class="btn" type="submit">{{ locale.hk_entrar_en_la_web_como|raw }} {{ user.username }}</button>
      </form>
      {% if hk.can('transaction/lookup') %}<a class="btn" href="{{ hk.path }}/transaction/lookup?user={{ user.username }}&days=0">{{ locale.hk_todas_sus_compras|raw }}</a>{% endif %}
      {% if hk.can('chatlog') %}<a class="btn" href="{{ hk.path }}/chatlogs?user={{ user.username }}">{{ locale.hk_todo_su_chat|raw }}</a>{% endif %}
    </div>
  </div>
  {% endif %}
</div>

{# ------------------------------------------------------------------ edit #}
{% if canEdit %}
<div class="tab-panel" data-panel="edit">
  <div class="card">
    <div class="card-head"><h2>{{ locale.hk_editar_a|raw }} {{ user.username }}</h2><span class="muted small">{{ locale.hk_mision_look_y_creditos_se_actualizan_en_el_juego|raw }}</span></div>
    <div class="card-body">
      <form class="form" method="post" action="{{ hk.path }}/users/save">
        <input type="hidden" name="hk_token" value="{{ hk.token }}">
        <input type="hidden" name="id" value="{{ user.id }}">
        <div class="row r-2">
          <div class="field"><label>{{ locale.hk_mision|raw }}</label><input type="text" name="motto" maxlength="100" value="{{ user.motto }}"></div>
          <div class="field"><label>{{ locale.hk_email|raw }}</label><input type="text" name="email" value="{{ user.email }}"></div>
        </div>
        <div class="row">
          <div class="field"><label>{{ locale.hk_creditos_2|raw }}</label><input type="number" name="credits" min="0" value="{{ user.credits }}"></div>
          <div class="field"><label>{{ locale.hk_pixeles_2|raw }}</label><input type="number" name="pixels" min="0" value="{{ user.pixels }}"></div>
          <div class="field"><label>{{ locale.hk_tickets_2|raw }}</label><input type="number" name="tickets" min="0" value="{{ user.tickets }}"></div>
          <div class="field"><label>{{ locale.hk_carretes_de_fotos|raw }}</label><input type="number" name="film" min="0" value="{{ user.film }}"></div>
          <div class="field">
            <label>{{ locale.hk_rango|raw }}</label>
            <select name="rank" {% if self %}disabled{% endif %}>
              {% for r in ranks %}<option value="{{ r }}" {% if r == user.rank %}selected{% endif %}>{{ r }} � {{ hk.rankLabel(r) }}</option>{% endfor %}
              {% if self %}<option value="{{ user.rank }}" selected>{{ user.rank }} � {{ hk.rankLabel(user.rank) }}</option>{% endif %}
            </select>
            {% if self %}<input type="hidden" name="rank" value="{{ user.rank }}"><span class="help">{{ locale.hk_no_puedes_cambiar_tu_propio_rango|raw }}</span>{% endif %}
          </div>
        </div>
        <div class="row r-2">
          <div class="field">
            <label>{{ locale.hk_look_figure|raw }}</label>
            <input type="text" name="figure" class="mono" value="{{ user.figure }}">
            <span class="help">{{ locale.hk_formato_hd_180_1_ch_210_66_lg_270_82|raw }}</span>
          </div>
          <div class="field">
            <label>{{ locale.hk_sexo|raw }}</label>
            <select name="sex"><option value="M" {% if user.sex != 'F' %}selected{% endif %}>{{ locale.hk_chico|raw }}</option><option value="F" {% if user.sex == 'F' %}selected{% endif %}>{{ locale.hk_chica|raw }}</option></select>
          </div>
        </div>
        <div class="row r-2">
          <div class="field">
            <label>{{ locale.hk_nueva_contrasena|raw }}</label>
            <input type="password" name="password" autocomplete="new-password" placeholder="{{ locale.hk_dejalo_vacio_para_no_cambiarla|raw }}">
          </div>
          <div class="field" style="justify-content:flex-end">
            <label class="check"><input type="checkbox" name="apply_now" value="1"> {{ locale.hk_si_esta_conectado_desconectarlo_para_aplicar_ya|raw }}</label>
          </div>
        </div>
        <div class="form-actions"><button class="btn primary" type="submit">{{ locale.hk_guardar_cambios|raw }}</button></div>
      </form>
    </div>
  </div>
</div>
{% endif %}

{# ------------------------------------------------------------------ moderation #}
{% if canManage and (canEdit or hk.can('bans')) %}
<div class="tab-panel" data-panel="moderation">
  <div class="grid g-2">
    {% if canEdit %}
    <div class="card">
      <div class="card-head"><h2>{{ locale.hk_economia|raw }}</h2><span class="muted small">{{ locale.hk_se_suma_o_resta_al_saldo_actual|raw }}</span></div>
      <div class="card-body form">
        <form class="form" method="post" action="{{ hk.path }}/users/action">
          <input type="hidden" name="hk_token" value="{{ hk.token }}"><input type="hidden" name="id" value="{{ user.id }}"><input type="hidden" name="do" value="credits"><input type="hidden" name="return" value="{{ back }}&tab=moderation">
          <div class="row r-2">
            <div class="field"><label>{{ locale.hk_creditos_3|raw }}</label><input type="number" name="amount" placeholder="{{ locale.hk_p_ej_500_o_200|raw }}" required></div>
            <div class="field"><label>{{ locale.hk_motivo_opcional|raw }}</label><input type="text" name="reason" placeholder="{{ locale.hk_premio_de_evento|raw }}"></div>
          </div>
          <div class="form-actions"><button class="btn primary" type="submit">{{ locale.hk_aplicar_creditos|raw }}</button><span class="help">{{ locale.hk_se_ve_en_el_juego_al_momento|raw }}</span></div>
        </form>
        <form class="form" method="post" action="{{ hk.path }}/users/action">
          <input type="hidden" name="hk_token" value="{{ hk.token }}"><input type="hidden" name="id" value="{{ user.id }}"><input type="hidden" name="do" value="pixels"><input type="hidden" name="return" value="{{ back }}&tab=moderation">
          <div class="row r-2">
            <div class="field"><label>{{ locale.hk_pixeles_3|raw }}</label><input type="number" name="amount" required></div>
            <div class="field" style="justify-content:flex-end"><button class="btn" type="submit">{{ locale.hk_aplicar_pixeles|raw }}</button></div>
          </div>
        </form>
        <form class="form" method="post" action="{{ hk.path }}/users/action">
          <input type="hidden" name="hk_token" value="{{ hk.token }}"><input type="hidden" name="id" value="{{ user.id }}"><input type="hidden" name="do" value="club"><input type="hidden" name="return" value="{{ back }}&tab=moderation">
          <div class="row r-2">
            <div class="field"><label>{{ locale.hk_dias_de_habbo_club|raw }}</label><input type="number" name="days" placeholder="31" required></div>
            <div class="field" style="justify-content:flex-end"><button class="btn" type="submit">{{ locale.hk_aplicar_hc|raw }}</button></div>
          </div>
        </form>
        {% if user.is_club %}
        <form method="post" action="{{ hk.path }}/users/action" data-confirm="{{ locale.hk_quitar_el_hc_a|raw }} {{ user.username }}?">
          <input type="hidden" name="hk_token" value="{{ hk.token }}"><input type="hidden" name="id" value="{{ user.id }}"><input type="hidden" name="do" value="club_remove"><input type="hidden" name="return" value="{{ back }}&tab=moderation">
          <button class="btn ghost-danger sm" type="submit">{{ locale.hk_quitar_hc_ahora|raw }}</button>
        </form>
        {% endif %}
      </div>
    </div>
    {% endif %}

    <div class="card">
      <div class="card-head"><h2>{{ locale.hk_en_el_hotel|raw }}</h2>{% if user.is_online %}<span class="pill ok">{{ locale.hk_conectado_2|raw }}</span>{% else %}<span class="pill">{{ locale.hk_desconectado|raw }}</span>{% endif %}</div>
      <div class="card-body form">
        {% if canEdit %}
        <form class="form" method="post" action="{{ hk.path }}/users/action">
          <input type="hidden" name="hk_token" value="{{ hk.token }}"><input type="hidden" name="id" value="{{ user.id }}"><input type="hidden" name="do" value="alert"><input type="hidden" name="return" value="{{ back }}&tab=moderation">
          <div class="field"><label>{{ locale.hk_mensaje_privado_alerta_en_el_juego|raw }}</label><textarea name="message" rows="3" maxlength="400" required placeholder="{{ locale.hk_hola_te_escribe_el_staff|raw }}"></textarea>
          <span class="help">{{ locale.hk_requiere_que_havana_server_tenga_el_comando_rcon|raw }} <code>{{ locale.hk_user_alert|raw }}</code> {{ locale.hk_incluido_en_el_codigo_se_activa_al_reconstruir_e|raw }}</span></div>
          <div class="form-actions"><button class="btn" type="submit">{{ locale.hk_enviar_alerta|raw }}</button></div>
        </form>
        {% if not self %}
        <form method="post" action="{{ hk.path }}/users/action" data-confirm="{{ locale.hk_desconectar_a|raw }} {{ user.username }} {{ locale.hk_del_hotel|raw }}">
          <input type="hidden" name="hk_token" value="{{ hk.token }}"><input type="hidden" name="id" value="{{ user.id }}"><input type="hidden" name="do" value="kick"><input type="hidden" name="return" value="{{ back }}&tab=moderation">
          <div class="form-actions"><button class="btn" type="submit" {% if not user.is_online %}disabled title="{{ locale.hk_no_esta_conectado|raw }}"{% endif %}>{{ locale.hk_expulsar_del_hotel_desconectar|raw }}</button></div>
        </form>
        {% endif %}
        {% endif %}
      </div>
    </div>
  </div>

  {% if hk.can('bans') and not self %}
  <div class="card danger-zone mt">
    <div class="card-head"><h2>{{ locale.hk_banear|raw }}</h2></div>
    <div class="card-body">
      {% if activeBan %}
        <div class="note warn mb">{{ locale.hk_este_usuario_ya_tiene_un_baneo_activo_puedes_ret|raw }}</div>
      {% endif %}
      <form class="form" method="post" action="{{ hk.path }}/users/action" data-confirm="{{ locale.hk_banear_a|raw }} {{ user.username }}{{ locale.hk_se_le_desconectara_del_hotel|raw }}">
        <input type="hidden" name="hk_token" value="{{ hk.token }}"><input type="hidden" name="id" value="{{ user.id }}"><input type="hidden" name="do" value="ban"><input type="hidden" name="return" value="{{ back }}&tab=moderation">
        <div class="row">
          <div class="field" style="grid-column: span 2"><label>{{ locale.hk_motivo_lo_vera_el_usuario|raw }}</label><input type="text" name="reason" maxlength="200" required></div>
          <div class="field">
            <label>{{ locale.hk_duracion|raw }}</label>
            <select name="hours">
              <option value="1">{{ locale.hk_1_hora|raw }}</option><option value="24" selected>{{ locale.hk_1_dia|raw }}</option><option value="72">{{ locale.hk_3_dias|raw }}</option>
              <option value="168">{{ locale.hk_1_semana|raw }}</option><option value="720">{{ locale.hk_30_dias|raw }}</option><option value="0">{{ locale.hk_permanente|raw }}</option>
            </select>
          </div>
        </div>
        <label class="check"><input type="checkbox" name="machine" value="1"> {{ locale.hk_banear_tambien_su_equipo_machine_id|raw }}{% if machineAccounts | length > 0 %} &#8212; <strong>{{ locale.hk_ojo_lo_comparte_con|raw }} {{ machineAccounts | length }} {{ locale.hk_cuenta_s|raw }}</strong>{% endif %}</label>
        <div class="form-actions"><button class="btn danger" type="submit">{{ locale.hk_banear_a_2|raw }} {{ user.username }}</button></div>
      </form>
    </div>
  </div>
  {% endif %}

  <div class="card flush mt">
    <div class="card-head"><h2>{{ locale.hk_historial_de_baneos|raw }}</h2></div>
    <div class="card-body scroll">
      {% if bans is empty %}<div class="empty">{{ locale.hk_sin_baneos|raw }}</div>{% else %}
      <table class="tbl">
        <thead><tr><th>{{ locale.hk_tipo|raw }}</th><th>{{ locale.hk_motivo|raw }}</th><th>{{ locale.hk_desde|raw }}</th><th>{{ locale.hk_hasta|raw }}</th><th>{{ locale.hk_por_2|raw }}</th><th>{{ locale.hk_estado|raw }}</th><th></th></tr></thead>
        <tbody>
        {% for b in bans %}
          <tr>
            <td>{% if b.ban_type == 'MACHINE_ID' %}{{ locale.hk_equipo|raw }}{% elseif b.ban_type == 'IP_ADDRESS' %}{{ locale.hk_ip|raw }}{% else %}{{ locale.hk_cuenta|raw }}{% endif %}</td>
            <td class="wrap">{{ b.message }}</td>
            <td class="nowrap muted">{{ b.banned_at }}</td>
            <td class="nowrap">{{ b.banned_until }}</td>
            <td>{{ b.banned_by }}</td>
            <td>{% if b.is_active %}<span class="pill bad">{{ locale.hk_activo_2|raw }}</span>{% else %}<span class="pill">{{ locale.hk_terminado|raw }}</span>{% endif %}</td>
            <td class="actions">
              {% if b.is_active and hk.can('bans') %}
              <form class="inline" method="post" action="{{ hk.path }}/bans/unban" data-confirm="{{ locale.hk_retirar_este_baneo|raw }}">
                <input type="hidden" name="hk_token" value="{{ hk.token }}"><input type="hidden" name="type" value="{{ b.ban_type }}"><input type="hidden" name="value" value="{{ b.banned_value }}"><input type="hidden" name="return" value="{{ back }}&tab=moderation">
                <button class="btn sm" type="submit">{{ locale.hk_desbanear|raw }}</button>
              </form>
              {% endif %}
            </td>
          </tr>
        {% endfor %}
        </tbody>
      </table>
      {% endif %}
    </div>
  </div>
</div>
{% endif %}

{# ------------------------------------------------------------------ badges #}
<div class="tab-panel" data-panel="badges">
  <div class="card">
    <div class="card-head"><h2>{{ locale.hk_placas|raw }}</h2><span class="muted small">{{ locale.hk_las_placas_nuevas_se_ven_en_el_juego_cuando_vuel|raw }}</span></div>
    <div class="card-body">
      {% if badges is empty %}<div class="empty">{{ locale.hk_no_tiene_placas|raw }}</div>{% else %}
      <div class="badges">
        {% for b in badges %}
          <div class="badge-chip {% if b.is_equipped %}eq{% endif %}" title="{% if b.is_equipped %}{{ locale.hk_puesta_ranura|raw }} {{ b.slot_id }}){% else %}{{ locale.hk_no_puesta|raw }}{% endif %}">
            <img class="badge-img" src="/c_images/album1584/{{ b.badge }}.gif" alt="" onerror="this.style.visibility='hidden'">
            <span class="mono">{{ b.badge }}</span>
            {% if hk.can('badges') and canManage %}
            <form method="post" action="{{ hk.path }}/users/action" data-confirm="{{ locale.hk_quitar_la_placa|raw }} {{ b.badge }}?">
              <input type="hidden" name="hk_token" value="{{ hk.token }}"><input type="hidden" name="id" value="{{ user.id }}"><input type="hidden" name="do" value="badge_remove"><input type="hidden" name="badge" value="{{ b.badge }}"><input type="hidden" name="return" value="{{ back }}&tab=badges">
              <button class="btn ghost-danger sm" type="submit">{{ locale.hk_quitar|raw }}</button>
            </form>
            {% endif %}
          </div>
        {% endfor %}
      </div>
      {% endif %}
    </div>
    {% if hk.can('badges') and canManage %}
    <div class="card-foot">
      <form class="toolbar" style="margin:0" method="post" action="{{ hk.path }}/users/action">
        <input type="hidden" name="hk_token" value="{{ hk.token }}"><input type="hidden" name="id" value="{{ user.id }}"><input type="hidden" name="do" value="badge_add"><input type="hidden" name="return" value="{{ back }}&tab=badges">
        <input type="text" name="badge" placeholder="{{ locale.hk_codigo_de_placa_p_ej_adm_hc1_ach_login1|raw }}" required style="width:320px">
        <button class="btn primary" type="submit">{{ locale.hk_dar_placa|raw }}</button>
        {% if hk.can('badges') %}<a class="small" href="{{ hk.path }}/badges">{{ locale.hk_ver_todas_las_placas_del_hotel|raw }}</a>{% endif %}
      </form>
    </div>
    {% endif %}
  </div>
</div>

{# ------------------------------------------------------------------ rooms #}
<div class="tab-panel" data-panel="rooms">
  <div class="card flush">
    <div class="card-body scroll">
      {% if rooms is empty %}<div class="empty">{{ locale.hk_no_tiene_salas|raw }}</div>{% else %}
      <table class="tbl">
        <thead><tr><th>{{ locale.hk_sala|raw }}</th><th>{{ locale.hk_modelo|raw }}</th><th>{{ locale.hk_acceso|raw }}</th><th class="num">{{ locale.hk_furnis|raw }}</th><th class="num">{{ locale.hk_dentro_2|raw }}</th><th></th></tr></thead>
        <tbody>
        {% for r in rooms %}
          <tr>
            <td><a href="{{ hk.path }}/rooms/view?id={{ r.id }}"><strong>{{ r.name }}</strong></a> <span class="muted small">#{{ r.id }}</span>{% if r.is_hidden %} <span class="pill warn">{{ locale.hk_oculta|raw }}</span>{% endif %}</td>
            <td class="mono">{{ r.model }}</td>
            <td>{% if r.accesstype == 1 %}{{ locale.hk_timbre|raw }}{% elseif r.accesstype == 2 %}{{ locale.hk_contrasena|raw }}{% else %}{{ locale.hk_abierta|raw }}{% endif %}</td>
            <td class="num">{{ r.item_count }}</td>
            <td class="num">{% if r.visitors_now > 0 %}<span class="pill ok">{{ r.visitors_now }}</span>{% else %}0{% endif %}</td>
            <td class="actions"><a class="btn sm" href="{{ hk.path }}/rooms/view?id={{ r.id }}">{{ locale.hk_ver|raw }}</a>{% if hk.can('rooms/manage') %}<a class="btn sm" href="{{ hk.path }}/rooms/edit?id={{ r.id }}">{{ locale.hk_editar|raw }}</a>{% endif %}</td>
          </tr>
        {% endfor %}
        </tbody>
      </table>
      {% endif %}
    </div>
  </div>
</div>

{# ------------------------------------------------------------------ inventory #}
<div class="tab-panel" data-panel="inventory">
  <div class="card flush">
    <div class="card-head"><h2>{{ locale.hk_inventario|raw }}</h2><span class="muted small">{{ inventoryTotals.in_hand | default(0) }} {{ locale.hk_en_la_mano|raw }} {{ inventoryTotals.placed | default(0) }} {{ locale.hk_colocados_en_salas|raw }} {{ inventoryTotals.total | default(0) }} {{ locale.hk_en_total|raw }}</span></div>
    <div class="card-body scroll">
      {% if inventory is empty %}<div class="empty">{{ locale.hk_no_tiene_furnis_en_la_mano|raw }}</div>{% else %}
      <table class="tbl">
        <thead><tr><th>{{ locale.hk_furni|raw }}</th><th>{{ locale.hk_nombre|raw }}</th><th class="num">{{ locale.hk_cantidad|raw }}</th></tr></thead>
        <tbody>
        {% for i in inventory %}<tr><td class="mono">{{ i.sprite }}</td><td>{{ i.name }}</td><td class="num">{{ i.amount }}</td></tr>{% endfor %}
        </tbody>
      </table>
      {% endif %}
    </div>
  </div>
</div>

{# ------------------------------------------------------------------ friends #}
<div class="tab-panel" data-panel="friends">
  <div class="card flush">
    <div class="card-body">
      {% if friends is empty %}<div class="empty">{{ locale.hk_no_tiene_amigos_en_la_consola|raw }}</div>{% else %}
      <ul class="list">
        {% for f in friends %}
        <li><div class="user-cell"><img class="avatar-head" src="/habbo-imaging/avatarimage?figure={{ f.figure }}&size=s&head=1&direction=3&head_direction=3" alt=""><a href="{{ hk.path }}/users/view?id={{ f.id }}">{{ f.username }}</a></div>
          {% if f.is_online %}<span class="pill ok">{{ locale.hk_conectado_2|raw }}</span>{% else %}<span class="muted small">{{ locale.hk_visto|raw }} {{ f.last_online }}</span>{% endif %}</li>
        {% endfor %}
      </ul>
      {% endif %}
    </div>
  </div>
</div>

{# ------------------------------------------------------------------ chat #}
<div class="tab-panel" data-panel="chat">
  <div class="card flush">
    <div class="card-head"><h2>{{ locale.hk_ultimos_mensajes|raw }}</h2>{% if hk.can('chatlog') %}<a class="small" href="{{ hk.path }}/chatlogs?user={{ user.username }}">{{ locale.hk_buscar_en_todo_su_chat|raw }}</a>{% endif %}</div>
    <div class="card-body">
      {% for c in chat %}
        <div class="chat-line">
          <span class="when">{{ c.said_at }}</span>
          <span><a href="{{ hk.path }}/rooms/view?id={{ c.room_id }}">{{ c.room_name | default(locale.hk_sala_2 ~ c.room_id) }}</a></span>
          <span class="msg">{% if c.chat_type == 1 %}<span class="pill">{{ locale.hk_grita|raw }}</span> {% elseif c.chat_type == 2 %}<span class="pill">{{ locale.hk_susurra|raw }}</span> {% endif %}{{ c.message }}</span>
        </div>
      {% else %}
        <div class="empty">{{ locale.hk_no_ha_escrito_nada_en_las_salas|raw }}</div>
      {% endfor %}
    </div>
  </div>
</div>

{# ------------------------------------------------------------------ transactions #}
<div class="tab-panel" data-panel="transactions">
  <div class="card flush">
    <div class="card-head"><h2>{{ locale.hk_ultimas_compras|raw }}</h2>{% if hk.can('transaction/lookup') %}<a class="small" href="{{ hk.path }}/transaction/lookup?user={{ user.username }}&days=0">{{ locale.hk_ver_todas|raw }}</a>{% endif %}</div>
    <div class="card-body scroll">
      {% if transactions is empty %}<div class="empty">{{ locale.hk_no_ha_comprado_nada|raw }}</div>{% else %}
      <table class="tbl">
        <thead><tr><th>{{ locale.hk_cuando|raw }}</th><th>{{ locale.hk_que|raw }}</th><th class="num">{{ locale.hk_cantidad|raw }}</th><th class="num">{{ locale.hk_creditos_2|raw }}</th><th class="num">{{ locale.hk_pixeles_2|raw }}</th></tr></thead>
        <tbody>
        {% for t in transactions %}
          <tr><td class="nowrap muted">{{ t.created_at }}</td><td class="wrap">{{ t.description }}</td><td class="num">{{ t.amount }}</td><td class="num">{{ t.credit_cost }}</td><td class="num">{{ t.pixel_cost }}</td></tr>
        {% endfor %}
        </tbody>
      </table>
      {% endif %}
    </div>
  </div>
</div>

{# ------------------------------------------------------------------ access #}
<div class="tab-panel" data-panel="access">
  <div class="grid g-2">
    <div class="card flush">
      <div class="card-head"><h2>{{ locale.hk_ips|raw }}</h2></div>
      <div class="card-body scroll">
        {% if ips is empty %}<div class="empty">{{ locale.hk_sin_registros_de_ip|raw }}</div>{% else %}
        <table class="tbl">
          <thead><tr><th>{{ locale.hk_ip|raw }}</th><th class="num">{{ locale.hk_veces|raw }}</th><th>{{ locale.hk_primera|raw }}</th><th>{{ locale.hk_ultima|raw }}</th><th class="num">{{ locale.hk_cuentas|raw }}</th></tr></thead>
          <tbody>
          {% for ip in ips %}
            <tr><td class="mono">{{ ip.ip_address }}</td><td class="num">{{ ip.times }}</td><td class="nowrap muted">{{ ip.first_seen }}</td><td class="nowrap muted">{{ ip.last_seen }}</td><td class="num">{{ ip.accounts }}</td></tr>
          {% endfor %}
          </tbody>
        </table>
        {% endif %}
      </div>
      <div class="card-foot help">{{ locale.hk_todas_las_conexiones_pasan_por_la_red_de_docker|raw }}</div>
    </div>
    <div class="card">
      <div class="card-head"><h2>{{ locale.hk_equipo_machine_id|raw }}</h2></div>
      <div class="card-body">
        <dl class="kv">
          <dt>{{ locale.hk_machine_id|raw }}</dt><dd class="mono">{% if user.machine_id != '' %}{{ user.machine_id }}{% else %}<span class="muted">&#8212;</span>{% endif %}</dd>
          <dt>{{ locale.hk_otras_cuentas|raw }}</dt><dd>{% for m in machineAccounts %}<a href="{{ hk.path }}/users/view?id={{ m.id }}">{{ m.username }}</a>{% if not loop.last %}, {% endif %}{% else %}<span class="muted">{{ locale.hk_ninguna|raw }}</span>{% endfor %}</dd>
        </dl>
      </div>
    </div>
  </div>
</div>

{# ------------------------------------------------------------------ history #}
<div class="tab-panel" data-panel="history">
  <div class="card flush">
    <div class="card-body">
      {% for a in audit %}
        <div class="chat-line">
          <span class="when">{{ a.created_at }}</span>
          <span><strong>{{ a.staff_name | default(locale.hk_sistema) }}</strong></span>
          <span class="msg"><span class="pill">{{ hk.actionLabel(a.action) }}</span> <span class="muted">{{ a.details }}</span></span>
        </div>
      {% else %}
        <div class="empty">{{ locale.hk_el_staff_no_ha_hecho_nada_con_esta_cuenta_todavi|raw }}</div>
      {% endfor %}
    </div>
  </div>
</div>
{% endautoescape %}
{% include "housekeeping/base/footer.tpl" %}
