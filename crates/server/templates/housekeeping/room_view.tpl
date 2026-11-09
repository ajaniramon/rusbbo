{% set title = locale.hk_sala_4 ~ room.name %}
{% include "housekeeping/base/header.tpl" %}
{% autoescape 'html' %}
<div class="page-head">
  <div>
    <h2 style="font-size:20px">{{ room.name }} <span class="muted small">#{{ room.id }}</span></h2>
    <p>{% if room.owner_name != '' %}{{ locale.hk_sala_de|raw }} <a href="{{ hk.path }}/users/view?id={{ room.owner_id }}">{{ room.owner_name }}</a>{% else %}{{ locale.hk_sala_publica|raw }}{% endif %} {{ locale.hk_modelo_2|raw }} <span class="mono">{{ room.model }}</span> · {{ room.category_name | default(locale.hk_sin_categoria) }}{% if room.description != '' %} · «{{ room.description }}»{% endif %}</p>
  </div>
  <div class="actions">
    {% if room.visitors_now > 0 %}<span class="pill ok">{{ room.visitors_now }} / {{ room.visitors_max }} {{ locale.hk_dentro|raw }}</span>{% else %}<span class="pill">{{ locale.hk_vacia_aforo|raw }} {{ room.visitors_max }}</span>{% endif %}
    {% if room.is_hidden_room %}<span class="pill warn">{{ locale.hk_oculta_del_navegador|raw }}</span>{% endif %}
    {% if hk.can('rooms/manage') %}<a class="btn" href="{{ hk.path }}/rooms/edit?id={{ room.id }}">{{ locale.hk_editar_sala_2|raw }}</a>{% endif %}
    {% if hk.can('chatlog') %}<a class="btn" href="{{ hk.path }}/chatlogs?room={{ room.id }}">{{ locale.hk_todo_su_chat|raw }}</a>{% endif %}
  </div>
</div>

<div class="grid g-side">
  <div class="card flush">
    <div class="card-head"><h2>{{ locale.hk_chat_de_la_sala|raw }}</h2><span class="muted small">{{ locale.hk_ultimos_80_mensajes|raw }}</span></div>
    <div class="card-body scroll-y">
      {% for c in chat %}
        <div class="chat-line">
          <span class="when">{{ c.said_at }}</span>
          <span><a href="{{ hk.path }}/users/view?id={{ c.user_id }}">{{ c.username }}</a></span>
          <span class="msg">{% if c.chat_type == 1 %}<span class="pill warn">{{ locale.hk_grita|raw }}</span> {% elseif c.chat_type == 2 %}<span class="pill info">{{ locale.hk_susurra|raw }}</span> {% endif %}{{ c.message }}</span>
        </div>
      {% else %}
        <div class="empty">{{ locale.hk_nadie_ha_hablado_aqui_todavia|raw }}</div>
      {% endfor %}
    </div>
  </div>

  <div>
    <div class="card flush">
      <div class="card-head"><h2>{{ locale.hk_dentro_ahora_2|raw }}</h2><span class="muted small">{{ locale.hk_segun_su_ultima_sala|raw }}</span></div>
      <div class="card-body">
        <ul class="list">
          {% for u in occupants %}
            <li><div class="user-cell"><img class="avatar-head" src="/habbo-imaging/avatarimage?figure={{ u.figure }}&size=s&head=1&direction=3&head_direction=3" alt=""><a href="{{ hk.path }}/users/view?id={{ u.id }}">{{ u.username }}</a></div></li>
          {% else %}
            <li class="muted">{% if room.visitors_now > 0 %}{{ locale.hk_hay|raw }} {{ room.visitors_now }} {{ locale.hk_dentro_pero_no_se_puede_saber_quien_entraron_ant|raw }}{% else %}{{ locale.hk_no_hay_nadie|raw }}{% endif %}</li>
          {% endfor %}
        </ul>
      </div>
    </div>

    <div class="card mt">
      <div class="card-head"><h2>{{ locale.hk_acciones|raw }}</h2></div>
      <div class="card-body form">
        <form class="form" method="post" action="{{ hk.path }}/rooms/action">
          <input type="hidden" name="hk_token" value="{{ hk.token }}"><input type="hidden" name="id" value="{{ room.id }}"><input type="hidden" name="do" value="alert">
          <div class="field"><label>{{ locale.hk_alerta_a_los_que_estan_en_la_sala|raw }}</label><textarea name="message" rows="2" maxlength="300" required></textarea>
          <span class="help">{{ locale.hk_requiere_el_comando_rcon|raw }} <code>{{ locale.hk_room_alert|raw }}</code> {{ locale.hk_incluido_en_el_codigo_se_activa_al_reconstruir_h|raw }}</span></div>
          <div class="form-actions"><button class="btn" type="submit">{{ locale.hk_enviar_a_la_sala|raw }}</button></div>
        </form>
        <form method="post" action="{{ hk.path }}/rooms/action" data-confirm="{{ locale.hk_se_desconectara_del_hotel_a_todos_los_que_esten|raw }}">
          <input type="hidden" name="hk_token" value="{{ hk.token }}"><input type="hidden" name="id" value="{{ room.id }}"><input type="hidden" name="do" value="kick_all">
          <button class="btn ghost-danger" type="submit" {% if occupants is empty %}disabled{% endif %}>{{ locale.hk_vaciar_la_sala_desconectar_a_quien_esta_dentro|raw }}</button>
        </form>
      </div>
    </div>
  </div>
</div>

<div class="grid g-2 mt">
  <div class="card flush">
    <div class="card-head"><h2>{{ locale.hk_furnis|raw }} <span class="sub">{{ itemCount }}</span></h2></div>
    <div class="card-body scroll scroll-y">
      {% if items is empty %}<div class="empty">{{ locale.hk_no_hay_furnis_colocados|raw }}</div>{% else %}
      <table class="tbl">
        <thead><tr><th>{{ locale.hk_furni|raw }}</th><th>{{ locale.hk_nombre|raw }}</th><th class="num">{{ locale.hk_cantidad|raw }}</th><th>{{ locale.hk_de_2|raw }}</th></tr></thead>
        <tbody>{% for i in items %}<tr><td class="mono">{{ i.sprite }}</td><td>{{ i.name }}</td><td class="num">{{ i.amount }}</td><td class="muted small">{{ i.owners }}</td></tr>{% endfor %}</tbody>
      </table>
      {% endif %}
    </div>
  </div>
  <div class="card">
    <div class="card-head"><h2>{{ locale.hk_detalles|raw }}</h2></div>
    <div class="card-body">
      <dl class="kv">
        <dt>{{ locale.hk_acceso|raw }}</dt><dd>{% if room.accesstype == 1 %}{{ locale.hk_con_timbre|raw }}{% elseif room.accesstype == 2 %}{{ locale.hk_con_contrasena|raw }}{% else %}{{ locale.hk_abierta|raw }}{% endif %}</dd>
        <dt>{{ locale.hk_puntuacion|raw }}</dt><dd>{{ room.rating }}</dd>
        <dt>{{ locale.hk_creada|raw }}</dt><dd>{{ room.created_at }}</dd>
        <dt>{{ locale.hk_con_derechos|raw }}</dt><dd>{% for r in rights %}<a href="{{ hk.path }}/users/view?id={{ r.userId() }}">{{ r.username() }}</a>{% if not loop.last %}, {% endif %}{% else %}<span class="muted">{{ locale.hk_nadie|raw }}</span>{% endfor %}</dd>
        <dt>{{ locale.hk_baneados_de_la_sala|raw }}</dt><dd>{% for b in bans %}{{ b.username() }}{% if not loop.last %}, {% endif %}{% else %}<span class="muted">{{ locale.hk_nadie|raw }}</span>{% endfor %}</dd>
        <dt>{{ locale.hk_placas_de_entrada|raw }}</dt><dd>{% for b in entryBadges %}<span class="pill">{{ b }}</span> {% else %}<span class="muted">{{ locale.hk_ninguna|raw }}</span>{% endfor %}</dd>
        <dt>{{ locale.hk_visitas_recientes|raw }}</dt><dd>{% for v in visits %}<a href="{{ hk.path }}/users/view?id={{ v.id }}" title="{{ v.visited_at }}">{{ v.username }}</a>{% if not loop.last %}, {% endif %}{% else %}<span class="muted">&#8212;</span>{% endfor %}</dd>
      </dl>
    </div>
  </div>
</div>

{% if audit is not empty %}
<div class="card flush mt">
  <div class="card-head"><h2>{{ locale.hk_historial_del_staff|raw }}</h2></div>
  <div class="card-body">
    {% for a in audit %}<div class="chat-line"><span class="when">{{ a.created_at }}</span><span><strong>{{ a.staff_name }}</strong></span><span class="msg"><span class="pill">{{ hk.actionLabel(a.action) }}</span> <span class="muted">{{ a.details }}</span></span></div>{% endfor %}
  </div>
</div>
{% endif %}
{% endautoescape %}
{% include "housekeeping/base/footer.tpl" %}
