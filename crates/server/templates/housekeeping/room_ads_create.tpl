{% set title = locale.hk_nuevo_anuncio_2 %}
{% include "housekeeping/base/header.tpl" %}
{% autoescape 'html' %}
<div class="page-head"><div></div><div class="actions"><a class="btn" href="{{ hk.path }}/room_ads">{{ locale.hk_volver_a_anuncios|raw }}</a></div></div>
<div class="card" style="max-width:720px">
  <div class="card-body">
    <form class="form" method="post" action="{{ hk.path }}/room_ads/create">
      <input type="hidden" name="hk_token" value="{{ hk.token }}">
      <div class="row r-2">
        <div class="field"><label>{{ locale.hk_id_de_la_sala|raw }}</label><input type="number" name="roomid" required></div>
        <div class="field"><label>{{ locale.hk_imagen|raw }}</label><input type="text" name="image" placeholder="{{ locale.hk_http_cartel_gif|raw }}"></div>
      </div>
      <div class="field"><label>{{ locale.hk_enlace_al_hacer_clic|raw }}</label><input type="text" name="url" placeholder="{{ locale.hk_http|raw }}"></div>
      <div class="form-actions">
        <label class="check"><input type="checkbox" name="enabled" checked> {{ locale.hk_activo_2|raw }}</label>
        <label class="check"><input type="checkbox" name="loading-ad"> {{ locale.hk_anuncio_de_carga_entre_salas|raw }}</label>
      </div>
      <div class="form-actions"><button class="btn primary" type="submit">{{ locale.hk_crear_anuncio|raw }}</button></div>
    </form>
  </div>
</div>
{% endautoescape %}
{% include "housekeeping/base/footer.tpl" %}
