{% set title = locale.hk_registro_de_chat %}
{% include "housekeeping/base/header.tpl" %}
{% autoescape 'html' %}
<div class="card mb">
  <div class="card-body">
    <form class="toolbar" style="margin:0" method="get" action="{{ hk.path }}/chatlogs">
      <input type="text" name="text" value="{{ text }}" placeholder="{{ locale.hk_texto_del_mensaje|raw }}">
      <input type="text" name="user" value="{{ user }}" placeholder="{{ locale.hk_usuario_exacto|raw }}" style="width:170px">
      <input type="number" name="room" value="{{ room }}" placeholder="{{ locale.hk_id_de_sala|raw }}" style="width:120px">
      <label class="small muted">{{ locale.hk_desde|raw }} <input type="date" name="from" value="{{ from }}" style="width:auto"></label>
      <label class="small muted">{{ locale.hk_hasta|raw }} <input type="date" name="to" value="{{ to }}" style="width:auto"></label>
      <button class="btn primary" type="submit">{{ locale.hk_filtrar_2|raw }}</button>
      {% if text != '' or user != '' or room != '' or from != '' or to != '' or around > 0 %}<a class="btn ghost" href="{{ hk.path }}/chatlogs">{{ locale.hk_limpiar|raw }}</a>{% endif %}
    </form>
  </div>
  {% if activeRooms is not empty %}
  <div class="card-foot small">
    <span class="muted">{{ locale.hk_salas_con_mas_chat_en_las_ultimas_24_h|raw }}</span>
    {% for r in activeRooms %}<a href="{{ hk.path }}/chatlogs?room={{ r.room_id }}">{{ r.room_name | default(locale.hk_sala_2 ~ r.room_id) }}</a> <span class="muted">({{ r.lines_count }})</span>{% if not loop.last %} · {% endif %}{% endfor %}
  </div>
  {% endif %}
</div>

{% if around > 0 %}
<div class="note mb">{{ locale.hk_conversacion_alrededor_del_mensaje|raw }}{{ around }} {{ locale.hk_en|raw }} {% if roomName != null %}<a href="{{ hk.path }}/rooms/view?id={{ roomName.id }}">{{ roomName.name }}</a>{% else %}{{ locale.hk_la_sala|raw }}{% endif %} {{ locale.hk_10_minutos_antes_y_despues_en_orden_cronologico|raw }}</div>
{% elseif roomName != null %}
<div class="note mb">{{ locale.hk_mostrando_la_sala|raw }} <a href="{{ hk.path }}/rooms/view?id={{ roomName.id }}">{{ roomName.name }}</a>.</div>
{% endif %}

<div class="card flush">
  <div class="card-body">
    {% for l in lines %}
      <div class="chat-line {% if l.id == around %}hl{% endif %}">
        <span class="when">{{ l.said_at }}</span>
        <span class="user-cell"><img class="avatar-head" style="width:26px;height:26px" src="/habbo-imaging/avatarimage?figure={{ l.figure }}&size=s&head=1&direction=3&head_direction=3" alt=""><a href="{{ hk.path }}/chatlogs?user={{ l.username }}">{{ l.username }}</a></span>
        <span class="msg">
          {% if l.chat_type == 1 %}<span class="pill warn">{{ locale.hk_grita|raw }}</span> {% elseif l.chat_type == 2 %}<span class="pill info">{{ locale.hk_susurra|raw }}</span> {% endif %}{{ l.message }}
          <span class="muted small"> · <a href="{{ hk.path }}/chatlogs?room={{ l.room_id }}">{{ l.room_name | default(locale.hk_sala_2 ~ l.room_id) }}</a>{% if around == 0 %} · <a href="{{ hk.path }}/chatlogs?around={{ l.id }}">{{ locale.hk_contexto|raw }}</a>{% endif %}</span>
        </span>
      </div>
    {% else %}
      <div class="empty">{{ locale.hk_no_hay_mensajes_con_esos_filtros|raw }}</div>
    {% endfor %}
  </div>
  {% include "housekeeping/base/pager.tpl" %}
</div>
{% endautoescape %}
{% include "housekeeping/base/footer.tpl" %}
