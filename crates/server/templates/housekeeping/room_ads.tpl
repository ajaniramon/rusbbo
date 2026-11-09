{% set title = locale.hk_anuncios_en_salas %}
{% include "housekeeping/base/header.tpl" %}
{% autoescape 'html' %}
<div class="page-head">
  <div><p>{{ locale.hk_carteles_publicitarios_billboards_que_aparecen_d|raw }} <code>{{ locale.hk_room_ads|raw }}</code>{% if gameConfig.getBoolean('room.ads') %} <span class="pill ok">{{ locale.hk_activado|raw }}</span>{% else %} <span class="pill warn">{{ locale.hk_desactivado|raw }}</span>{% endif %}.</p></div>
  <div class="actions"><a class="btn primary" href="{{ hk.path }}/room_ads/create">{{ locale.hk_nuevo_anuncio|raw }}</a></div>
</div>
<div class="card flush">
  {% if roomAds is empty %}
    <div class="card-body"><div class="empty">{{ locale.hk_no_hay_anuncios|raw }}</div></div>
  {% else %}
  <form method="post" action="{{ hk.path }}/room_ads" id="ads-form">
    <input type="hidden" name="hk_token" value="{{ hk.token }}">
    {% for ad in roomAds %}<input type="hidden" name="roomad-id-{{ ad.getId() }}" value="{{ ad.getId() }}">{% endfor %}
  </form>
  <div class="card-body scroll">
    <table class="tbl">
      <thead><tr><th>{{ locale.hk_activo_2|raw }}</th><th>{{ locale.hk_de_carga|raw }}</th><th>{{ locale.hk_id_sala|raw }}</th><th>{{ locale.hk_enlace_url|raw }}</th><th>{{ locale.hk_imagen|raw }}</th><th></th></tr></thead>
      <tbody>
      {% for ad in roomAds %}
        <tr>
          <td><input type="checkbox" form="ads-form" name="roomad-{{ ad.getId() }}-enabled" {% if ad.isEnabled() %}checked{% endif %}></td>
          <td><input type="checkbox" form="ads-form" name="roomad-{{ ad.getId() }}-loading-ad" {% if ad.isLoadingAd() %}checked{% endif %}></td>
          <td style="width:110px"><input type="number" form="ads-form" name="roomad-{{ ad.getId() }}-roomid" value="{{ ad.getRoomId() }}"></td>
          <td><input type="text" form="ads-form" name="roomad-{{ ad.getId() }}-url" value="{{ ad.getUrl() }}"></td>
          <td><input type="text" form="ads-form" name="roomad-{{ ad.getId() }}-image" value="{{ ad.getImage() }}"></td>
          <td class="actions">
            <form class="inline" method="post" action="{{ hk.path }}/room_ads/delete?id={{ ad.getId() }}" data-confirm="{{ locale.hk_borrar_este_anuncio|raw }}">
              <input type="hidden" name="hk_token" value="{{ hk.token }}"><button class="btn sm ghost-danger" type="submit">{{ locale.hk_borrar|raw }}</button>
            </form>
          </td>
        </tr>
      {% endfor %}
      </tbody>
    </table>
  </div>
  <div class="card-foot"><button class="btn primary" type="submit" form="ads-form">{{ locale.hk_guardar_cambios|raw }}</button></div>
  {% endif %}
</div>
{% endautoescape %}
{% include "housekeeping/base/footer.tpl" %}
