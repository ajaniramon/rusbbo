{% set title = locale.hk_nueva_placa_de_entrada_2 %}
{% include "housekeeping/base/header.tpl" %}
{% autoescape 'html' %}
<div class="page-head"><div></div><div class="actions"><a class="btn" href="{{ hk.path }}/room_badges">{{ locale.hk_volver|raw }}</a></div></div>
<div class="card" style="max-width:620px">
  <div class="card-body">
    <form class="form" method="post" action="{{ hk.path }}/room_badges/create">
      <input type="hidden" name="hk_token" value="{{ hk.token }}">
      <div class="row r-2">
        <div class="field"><label>{{ locale.hk_id_de_la_sala|raw }}</label><input type="number" name="roomid" required></div>
        <div class="field"><label>{{ locale.hk_codigo_de_placa|raw }}</label><input type="text" name="badgecode" class="mono" required placeholder="{{ locale.hk_p_ej_event1|raw }}"></div>
      </div>
      <p class="help" style="margin:0">{{ locale.hk_quien_entre_en_la_sala_recibira_la_placa_si_no_l|raw }}</p>
      <div class="form-actions"><button class="btn primary" type="submit">{{ locale.hk_crear|raw }}</button></div>
    </form>
  </div>
</div>
{% endautoescape %}
{% include "housekeeping/base/footer.tpl" %}
