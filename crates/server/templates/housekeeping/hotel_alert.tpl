{% set title = locale.hk_alerta_al_hotel %}
{% include "housekeeping/base/header.tpl" %}
{% autoescape 'html' %}
<div class="grid g-side">
  <div class="card">
    <div class="card-head"><h2>{{ locale.hk_mensaje_para_todos_los_conectados|raw }}</h2></div>
    <div class="card-body">
      <form class="form" method="post" action="{{ hk.path }}/hotel/alert" data-confirm="{{ locale.hk_enviar_esta_alerta_a_todo_el_hotel|raw }}">
        <input type="hidden" name="hk_token" value="{{ hk.token }}">
        <div class="field">
          <label>{{ locale.hk_mensaje|raw }}</label>
          <textarea name="message" rows="5" maxlength="500" required placeholder="{{ locale.hk_esta_noche_a_las_22_00_hay_fiesta_en_la_piscina|raw }}"></textarea>
          <span class="help">{{ locale.hk_aparece_como_ventana_de_alerta_en_el_cliente_de|raw }}</span>
        </div>
        <label class="check"><input type="checkbox" name="anonymous" value="1"> {{ locale.hk_firmar_como_staff_en_lugar_de_con_mi_nombre|raw }}</label>
        <div class="form-actions">
          <button class="btn primary" type="submit" {% if not hk.serverOnline %}disabled{% endif %}>{{ locale.hk_enviar_alerta|raw }}</button>
          {% if not hk.serverOnline %}<span class="pill bad">{{ locale.hk_el_servidor_de_juego_no_responde|raw }}</span>{% endif %}
        </div>
      </form>
    </div>
  </div>
  <div class="card flush">
    <div class="card-head"><h2>{{ locale.hk_la_recibiran|raw }} <span class="sub">{{ online | length }}</span></h2></div>
    <div class="card-body">
      <ul class="list">
        {% for u in online %}
          <li><div class="user-cell"><img class="avatar-head" src="/habbo-imaging/avatarimage?figure={{ u.figure }}&size=s&head=1&direction=3&head_direction=3" alt="">{{ u.username }}</div></li>
        {% else %}
          <li class="muted">{{ locale.hk_ahora_mismo_no_hay_nadie_conectado|raw }}</li>
        {% endfor %}
      </ul>
    </div>
  </div>
</div>

<div class="card flush mt">
  <div class="card-head"><h2>{{ locale.hk_alertas_enviadas|raw }}</h2></div>
  <div class="card-body">
    {% for h in history %}
      <div class="chat-line"><span class="when">{{ h.created_at }}</span><span><strong>{{ h.staff_name }}</strong></span><span class="msg">{{ h.details }}</span></div>
    {% else %}
      <div class="empty">{{ locale.hk_todavia_no_se_ha_enviado_ninguna_alerta_desde_el|raw }}</div>
    {% endfor %}
  </div>
</div>
{% endautoescape %}
{% include "housekeeping/base/footer.tpl" %}
