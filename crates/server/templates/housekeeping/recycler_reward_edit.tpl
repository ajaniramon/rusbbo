{% set title = (reward != null) ? (locale.hk_editar_premio ~ reward.sprite()) : locale.hk_nuevo_premio_del_reciclador %}
{% include "housekeeping/base/header.tpl" %}
{% autoescape 'html' %}
<div class="page-head">
  <div><p>{{ locale.hk_los_cambios_se_envian_al_juego_al_momento|raw }}</p></div>
  <div class="actions"><a class="btn" href="{{ hk.path }}/recycler_rewards">{{ locale.hk_volver|raw }}</a></div>
</div>
<div class="card" style="max-width:640px">
  <div class="card-body">
    <form class="form" method="post" action="{{ hk.path }}/recycler_rewards/edit{% if reward != null %}?sprite={{ reward.sprite() }}{% endif %}">
      <input type="hidden" name="hk_token" value="{{ hk.token }}">
      <div class="field"><label>{{ locale.hk_sprite_del_furni|raw }}</label><input type="text" name="sprite" class="mono" required value="{% if reward != null %}{{ reward.sprite() }}{% endif %}"></div>
      <div class="row r-2">
        <div class="field"><label>{{ locale.hk_orden|raw }}</label><input type="number" name="order_id" value="{% if reward != null %}{{ reward.orderId() }}{% else %}0{% endif %}"></div>
        <div class="field"><label>{{ locale.hk_nivel_de_premio|raw }}</label>
          <select name="chance">
            {% set current = (reward != null) ? reward.chance() : 5 %}
            <option value="5" {% if current == 5 %}selected{% endif %}>{{ locale.hk_comun_1_entre_5|raw }}</option>
            <option value="40" {% if current == 40 %}selected{% endif %}>{{ locale.hk_poco_comun_1_entre_40|raw }}</option>
            <option value="200" {% if current == 200 %}selected{% endif %}>{{ locale.hk_raro_1_entre_200|raw }}</option>
            <option value="2000" {% if current == 2000 %}selected{% endif %}>{{ locale.hk_muy_raro_1_entre_2000|raw }}</option>
            {% if current != 5 and current != 40 and current != 200 and current != 2000 %}<option value="{{ current }}" selected>{{ locale.hk_otro|raw }} {{ current }} {{ locale.hk_no_aparece_en_el_reciclador|raw }}</option>{% endif %}
          </select>
          <span class="help">{{ locale.hk_el_reciclador_solo_muestra_los_niveles_5_40_200|raw }}</span>
        </div>
      </div>
      <div class="form-actions"><button class="btn primary" type="submit">{% if reward != null %}{{ locale.hk_guardar_premio|raw }}{% else %}{{ locale.hk_crear_premio|raw }}{% endif %}</button></div>
    </form>
  </div>
</div>
{% endautoescape %}
{% include "housekeeping/base/footer.tpl" %}
