{% set title = locale.hk_panel %}
{% include "housekeeping/base/header.tpl" %}
{% autoescape 'html' %}
<div class="page-head">
  <div>
    <h2 style="font-size:20px">{{ locale.hk_hola|raw }} {{ hk.staff.getName() }}</h2>
    <p>{{ locale.hk_asi_esta|raw }} {{ site.siteName }} {{ locale.hk_ahora_mismo|raw }}</p>
  </div>
  <div class="actions">
    {% if hk.can('hotel/alert') %}<a class="btn" href="{{ hk.path }}/hotel/alert">{{ locale.hk_enviar_alerta_al_hotel|raw }}</a>{% endif %}
    {% if hk.can('user/search') %}
    <form class="inline" method="get" action="{{ hk.path }}/users">
      <input type="search" name="q" placeholder="{{ locale.hk_buscar_usuario|raw }}" style="width:220px">
    </form>
    {% endif %}
  </div>
</div>

{% if stats.maintenance %}<div class="flash warning"><span class="icon">!</span><span class="msg">{{ locale.hk_la_web_esta_en|raw }} <strong>{{ locale.hk_modo_mantenimiento|raw }}</strong> {{ locale.hk_ajuste|raw }} <code>{{ locale.hk_maintenance|raw }}</code>).</span></div>{% endif %}
{% if not stats.serverOnline %}<div class="flash danger"><span class="icon">!</span><span class="msg">{{ locale.hk_el_servidor_de_juego_no_responde_por_rcon_las_ac|raw }}</span></div>{% endif %}

<div class="stats">
  <div class="stat accent">
    <div class="label">{{ locale.hk_conectados_ahora|raw }}</div>
    <div class="value">{{ stats.online }}</div>
    <div class="sub">{{ locale.hk_pico_hoy|raw }} {{ stats.dailyPeak }} {{ locale.hk_record|raw }} {{ stats.allTimePeak }}</div>
  </div>
  <div class="stat">
    <div class="label">{{ locale.hk_salas_con_gente|raw }}</div>
    <div class="value">{{ stats.occupiedRooms }}</div>
    <div class="sub">{{ stats.rooms }} {{ locale.hk_salas_de_usuarios|raw }} {{ stats.publicRooms }} {{ locale.hk_publicas|raw }}</div>
  </div>
  <div class="stat">
    <div class="label">{{ locale.hk_usuarios|raw }}</div>
    <div class="value">{{ stats.users }}</div>
    <div class="sub">+{{ stats.usersToday }} {{ locale.hk_hoy|raw }}{{ stats.usersWeek }} {{ locale.hk_en_7_dias|raw }} {{ stats.activeToday }} {{ locale.hk_activos_hoy|raw }}</div>
  </div>
  <div class="stat">
    <div class="label">{{ locale.hk_creditos_en_circulacion|raw }}</div>
    <div class="value">{{ stats.credits | numberformat('#,##0') }}</div>
    <div class="sub">{{ stats.pixels | numberformat('#,##0') }} {{ locale.hk_pixeles|raw }} {{ stats.tickets }} {{ locale.hk_tickets|raw }}</div>
  </div>
  <div class="stat">
    <div class="label">{{ locale.hk_gastado_en_el_catalogo_7_dias|raw }}</div>
    <div class="value">{{ stats.spentWeek | numberformat('#,##0') }}</div>
    <div class="sub">{{ locale.hk_creditos|raw }} {{ stats.pixelsSpentWeek }} {{ locale.hk_pixeles|raw }} {{ stats.purchasesToday }} {{ locale.hk_compras_hoy|raw }}</div>
  </div>
  <div class="stat">
    <div class="label">{{ locale.hk_socios_hc|raw }}</div>
    <div class="value">{{ stats.clubMembers }}</div>
    <div class="sub">{{ stats.activeBans }} {{ locale.hk_baneo|raw }}{% if stats.activeBans != 1 %}{{ locale.hk_pl_baneo|raw }}{% endif %} {{ locale.hk_activo|raw }}{% if stats.activeBans != 1 %}{{ locale.hk_pl_activo|raw }}{% endif %} � {{ stats.chatToday }} {{ locale.hk_mensajes_hoy|raw }}</div>
  </div>
</div>

<div class="grid g-side mt">
  <div class="card flush">
    <div class="card-head"><h2>{{ locale.hk_conectados|raw }} <span class="sub">{{ onlineUsers | length }}</span></h2>{% if hk.can('user/search') %}<a class="small" href="{{ hk.path }}/users?filter=online">{{ locale.hk_ver_todos|raw }}</a>{% endif %}</div>
    <div class="card-body scroll">
      {% if onlineUsers is empty %}
        <div class="empty">{{ locale.hk_no_hay_nadie_conectado_ahora_mismo|raw }}</div>
      {% else %}
      <table class="tbl">
        <thead><tr><th>{{ locale.hk_usuario|raw }}</th><th>{{ locale.hk_donde_esta|raw }}</th><th class="num">{{ locale.hk_creditos_2|raw }}</th><th class="num">{{ locale.hk_pixeles_2|raw }}</th><th></th></tr></thead>
        <tbody>
        {% for u in onlineUsers %}
          <tr>
            <td><div class="user-cell"><img class="avatar-head" src="/habbo-imaging/avatarimage?figure={{ u.figure }}&size=s&head=1&direction=3&head_direction=3" alt=""><div><a href="{{ hk.path }}/users/view?id={{ u.id }}"><strong>{{ u.username }}</strong></a>{% if u.rank > 1 %} <span class="pill accent">{{ hk.rankLabel(u.rank) }}</span>{% endif %}<div class="muted small">{{ u.motto }}</div></div></div></td>
            <td>{% if u.room_id > 0 %}<a href="{{ hk.path }}/rooms/view?id={{ u.room_id }}">{{ u.room_name }}</a>{% else %}<span class="muted">{{ locale.hk_vista_del_hotel|raw }}</span>{% endif %}</td>
            <td class="num">{{ u.credits }}</td>
            <td class="num">{{ u.pixels }}</td>
            <td class="actions">{% if hk.can('user/search') %}<a class="btn sm" href="{{ hk.path }}/users/view?id={{ u.id }}">{{ locale.hk_ficha|raw }}</a>{% endif %}</td>
          </tr>
        {% endfor %}
        </tbody>
      </table>
      {% endif %}
    </div>
  </div>

  <div class="card flush">
    <div class="card-head"><h2>{{ locale.hk_salas_con_gente|raw }}</h2>{% if hk.can('rooms/view') %}<a class="small" href="{{ hk.path }}/rooms?filter=occupied">{{ locale.hk_ver_salas|raw }}</a>{% endif %}</div>
    <div class="card-body">
      {% if activeRooms is empty %}
        <div class="empty">{{ locale.hk_todas_las_salas_estan_vacias|raw }}</div>
      {% else %}
      <ul class="list">
        {% for r in activeRooms %}
        <li>
          <div><a href="{{ hk.path }}/rooms/view?id={{ r.id }}"><strong>{{ r.name }}</strong></a><div class="muted small">{% if r.owner_id == '0' or r.owner_name == '' %}{{ locale.hk_sala_publica|raw }}{% else %}{{ locale.hk_de|raw }} {{ r.owner_name }}{% endif %} � {{ r.model }}</div></div>
          <span class="pill ok">{{ r.visitors_now }} / {{ r.visitors_max }}</span>
        </li>
        {% endfor %}
      </ul>
      {% endif %}
    </div>
  </div>
</div>

<div class="grid g-2 mt">
  <div class="card flush">
    <div class="card-head"><h2>{{ locale.hk_ultimas_compras|raw }}</h2>{% if hk.can('transaction/lookup') %}<a class="small" href="{{ hk.path }}/transaction/lookup">{{ locale.hk_ver_todas|raw }}</a>{% endif %}</div>
    <div class="card-body scroll">
      {% if recentTransactions is empty %}<div class="empty">{{ locale.hk_todavia_no_hay_compras|raw }}</div>{% else %}
      <table class="tbl">
        <thead><tr><th>{{ locale.hk_cuando|raw }}</th><th>{{ locale.hk_usuario|raw }}</th><th>{{ locale.hk_que|raw }}</th><th class="num">{{ locale.hk_coste|raw }}</th></tr></thead>
        <tbody>
        {% for t in recentTransactions %}
          <tr>
            <td class="nowrap muted">{{ t.created_at }}</td>
            <td><a href="{{ hk.path }}/users/view?id={{ t.user_id }}">{{ t.username }}</a></td>
            <td class="wrap">{{ t.description }}{% if t.amount > 1 %} <span class="muted">{{ locale.hk_text|raw }}{{ t.amount }}</span>{% endif %}</td>
            <td class="num">{% if t.credit_cost != 0 %}{{ t.credit_cost }} {{ locale.hk_c|raw }}{% endif %}{% if t.pixel_cost != 0 %} {{ t.pixel_cost }} {{ locale.hk_px|raw }}{% endif %}{% if t.credit_cost == 0 and t.pixel_cost == 0 %}<span class="muted">{{ locale.hk_gratis|raw }}</span>{% endif %}</td>
          </tr>
        {% endfor %}
        </tbody>
      </table>
      {% endif %}
    </div>
  </div>

  <div class="card flush">
    <div class="card-head"><h2>{{ locale.hk_ultimos_registros|raw }}</h2>{% if hk.can('user/search') %}<a class="small" href="{{ hk.path }}/users">{{ locale.hk_ver_usuarios|raw }}</a>{% endif %}</div>
    <div class="card-body scroll">
      <table class="tbl">
        <thead><tr><th>{{ locale.hk_usuario|raw }}</th><th>{{ locale.hk_registrado|raw }}</th><th>{{ locale.hk_ultima_vez|raw }}</th><th class="num">{{ locale.hk_creditos_2|raw }}</th></tr></thead>
        <tbody>
        {% for u in recentUsers %}
          <tr>
            <td><div class="user-cell"><img class="avatar-head" src="/habbo-imaging/avatarimage?figure={{ u.figure }}&size=s&head=1&direction=3&head_direction=3" alt=""><a href="{{ hk.path }}/users/view?id={{ u.id }}">{{ u.username }}</a>{% if u.is_online %} <span class="dot ok" title="{{ locale.hk_conectado_2|raw }}"></span>{% endif %}</div></td>
            <td class="nowrap muted">{{ u.created_at }}</td>
            <td class="nowrap muted">{{ u.last_online }}</td>
            <td class="num">{{ u.credits }}</td>
          </tr>
        {% endfor %}
        </tbody>
      </table>
    </div>
  </div>
</div>

<div class="grid g-2 mt">
  <div class="card flush">
    <div class="card-head"><h2>{{ locale.hk_chat_reciente|raw }}</h2>{% if hk.can('chatlog') %}<a class="small" href="{{ hk.path }}/chatlogs">{{ locale.hk_abrir_el_registro_de_chat|raw }}</a>{% endif %}</div>
    <div class="card-body">
      {% for c in recentChat %}
        <div class="chat-line">
          <span class="when">{{ c.said_at }}</span>
          <span><a href="{{ hk.path }}/users/view?id={{ c.user_id }}">{{ c.username }}</a></span>
          <span class="msg">{{ c.message }} <a class="muted small" href="{{ hk.path }}/rooms/view?id={{ c.room_id }}">� {{ c.room_name }}</a></span>
        </div>
      {% else %}
        <div class="empty">{{ locale.hk_sin_mensajes_todavia|raw }}</div>
      {% endfor %}
    </div>
  </div>

  <div class="card flush">
    <div class="card-head"><h2>{{ locale.hk_acciones_del_staff|raw }}</h2>{% if hk.can('audit') %}<a class="small" href="{{ hk.path }}/audit">{{ locale.hk_ver_el_registro|raw }}</a>{% endif %}</div>
    <div class="card-body">
      {% for a in recentActions %}
        <div class="chat-line">
          <span class="when">{{ a.created_at }}</span>
          <span><strong>{{ a.staff_name | default(locale.hk_sistema) }}</strong></span>
          <span class="msg"><span class="pill">{{ hk.actionLabel(a.action) }}</span> {% if a.target_name != '' %}{{ a.target_name }} � {% endif %}<span class="muted">{{ a.details }}</span></span>
        </div>
      {% else %}
        <div class="empty">{{ locale.hk_todavia_no_hay_acciones_registradas|raw }}</div>
      {% endfor %}
    </div>
  </div>
</div>

<div class="card mt">
  <div class="card-head"><h2>{{ locale.hk_contenido_del_hotel|raw }}</h2></div>
  <div class="card-body">
    <dl class="kv">
      <dt>{{ locale.hk_furnis_colocados|raw }}</dt><dd>{{ stats.roomItems }}</dd>
      <dt>{{ locale.hk_furnis_en_inventarios|raw }}</dt><dd>{{ stats.inventoryItems }}</dd>
      <dt>{{ locale.hk_grupos|raw }}</dt><dd>{{ stats.groups }}</dd>
      <dt>{{ locale.hk_mascotas|raw }}</dt><dd>{{ stats.pets }}</dd>
      <dt>{{ locale.hk_fotos|raw }}</dt><dd>{{ stats.photos }}</dd>
      <dt>{{ locale.hk_registro|raw }}</dt><dd>{% if stats.registrationDisabled %}<span class="pill warn">{{ locale.hk_cerrado|raw }}</span>{% else %}<span class="pill ok">{{ locale.hk_abierto|raw }}</span>{% endif %}</dd>
    </dl>
  </div>
</div>
{% endautoescape %}
{% include "housekeeping/base/footer.tpl" %}
