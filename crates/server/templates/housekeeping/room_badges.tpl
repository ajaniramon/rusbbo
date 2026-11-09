{% set title = locale.hk_placas_de_entrada %}
{% include "housekeeping/base/header.tpl" %}
{% autoescape 'html' %}
<div class="page-head">
  <div><p>{{ locale.hk_placas_que_se_regalan_automaticamente_al_entrar|raw }}</p></div>
  <div class="actions"><a class="btn primary" href="{{ hk.path }}/room_badges/create">{{ locale.hk_nueva_placa_de_entrada|raw }}</a></div>
</div>
<div class="card flush">
  {% if roomBadges is empty %}
    <div class="card-body"><div class="empty">{{ locale.hk_ninguna_sala_regala_placas|raw }}</div></div>
  {% else %}
  <form method="post" action="{{ hk.path }}/room_badges" id="rb-form">
    <input type="hidden" name="hk_token" value="{{ hk.token }}">
  </form>
  <div class="card-body scroll">
    <table class="tbl">
      <thead><tr><th>{{ locale.hk_placa|raw }}</th><th>{{ locale.hk_id_sala|raw }}</th><th>{{ locale.hk_codigo|raw }}</th><th>{{ locale.hk_sala|raw }}</th><th></th></tr></thead>
      <tbody>
      {% for entry in roomBadges.entrySet() %}
        {% for badge in entry.getValue() %}
          {% set key = (entry.getKey()) ~ '_' ~ badge %}
          <tr>
            <td><img class="badge-img" src="/c_images/album1584/{{ badge }}.gif" alt="" onerror="this.style.visibility='hidden'"><input type="hidden" form="rb-form" name="roombadge-id-{{ key }}" value="{{ key }}"></td>
            <td style="width:120px"><input type="number" form="rb-form" name="roomad-{{ key }}-roomid" value="{{ entry.getKey() }}"></td>
            <td><input type="text" form="rb-form" class="mono" name="roomad-{{ key }}-badge" value="{{ badge }}"></td>
            <td><a href="{{ hk.path }}/rooms/view?id={{ entry.getKey() }}">{{ util.getRoomName(entry.getKey()) }}</a></td>
            <td class="actions">
              <form class="inline" method="post" action="{{ hk.path }}/room_badges/delete?id={{ key }}" data-confirm="{{ locale.hk_quitar_esta_placa_de_entrada|raw }}">
                <input type="hidden" name="hk_token" value="{{ hk.token }}"><button class="btn sm ghost-danger" type="submit">{{ locale.hk_quitar|raw }}</button>
              </form>
            </td>
          </tr>
        {% endfor %}
      {% endfor %}
      </tbody>
    </table>
  </div>
  <div class="card-foot"><button class="btn primary" type="submit" form="rb-form">{{ locale.hk_guardar_cambios|raw }}</button></div>
  {% endif %}
</div>
{% endautoescape %}
{% include "housekeeping/base/footer.tpl" %}
